// The picture of a selection being carried on the PCB canvas: which items are in the hand, and the one transform that shows them where the drop
// would put them.
//
// `EDIT_TOOL::doMoveSelection` moves the real items under the cursor and `Rotate` / `Flip` act on them in place, about the selection's reference
// point, which travels with the cursor. Nothing is committed until the drop, so this app keeps the items where they are and draws them through
// a transform instead: the same turn and flip about where the selection was picked up, then the move -- exactly the batch `planCarry`
// (kicad-port/pcbTransform.ts) sends on the drop, so what is drawn is what lands.
//
// Pure: no store, no DOM. Unit-tested in pcbCarry.test.ts.

import type { BoardState } from "../api/types";
import { padParent } from "./pcbItems";
import { groupAndDescendants, groupLeaves } from "./groupTree";

export type Matrix = readonly [number, number, number, number, number, number];

/** What the painter needs of a `MovePreview` (state/store.tsx) to carry a PCB selection. */
export interface CarryPreview {
  refs: readonly string[];
  dxUm: number;
  dyUm: number;
  /** R presses, counter-clockwise quarter turns (Shift+R takes one away). */
  rotateQuarterTurns?: number;
  flipped?: boolean;
  /** Where the selection was picked up: the point a turn is about, and the point a flip mirrors about. */
  pivotUm?: readonly [number, number];
  flipPivotUm?: readonly [number, number];
}

const IDENTITY: Matrix = [1, 0, 0, 1, 0, 0];

/** `a` applied after `b`, in the canvas order `[a, b, c, d, e, f]` (`x' = a x + c y + e`, `y' = b x + d y + f`). */
function after(a: Matrix, b: Matrix): Matrix {
  return [a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1], a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3], a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5]];
}

/** `quarterTurnsCcw` turns counter-clockwise on the screen about `(px, py)` (the canvas' `rotate` runs clockwise on a Y-down screen, hence the sign). */
function turnMatrix(quarterTurnsCcw: number, px: number, py: number): Matrix {
  const q = ((Math.round(quarterTurnsCcw) % 4) + 4) % 4;
  // cos and sin of -q * 90 degrees, exactly
  const [cos, sin] = q === 0 ? [1, 0] : q === 1 ? [0, -1] : q === 2 ? [-1, 0] : [0, 1];
  return [cos, sin, -sin, cos, px - cos * px + sin * py, py - sin * px - cos * py];
}

/** The mirror about the vertical line through `fx` (`FLIP_DIRECTION::LEFT_RIGHT`). */
function flipMatrix(fx: number): Matrix {
  return [-1, 0, 0, 1, 2 * fx, 0];
}

/** The transform from where the carried items are to where they are drawn: turned, then flipped, then moved. */
export function carryMatrix(p: CarryPreview): Matrix {
  let m: Matrix = IDENTITY;
  if (p.rotateQuarterTurns && p.rotateQuarterTurns % 4 !== 0) {
    const [px, py] = p.pivotUm ?? [0, 0];
    m = after(turnMatrix(p.rotateQuarterTurns, px, py), m);
  }
  if (p.flipped) m = after(flipMatrix((p.flipPivotUm ?? [0, 0])[0]), m);
  return after([1, 0, 0, 1, p.dxUm, p.dyUm], m);
}

/** Where a point of the carried items is drawn. */
export function carryPoint(p: CarryPreview, pt: readonly [number, number]): [number, number] {
  const m = carryMatrix(p);
  return [m[0] * pt[0] + m[2] * pt[1] + m[4], m[1] * pt[0] + m[3] * pt[1] + m[5]];
}

/**
 * The ids a carry moves: the selection with every pad standing for its footprint and every group for all its members (the group itself is
 * kept, its box moves with it).
 */
export function carriedIds(board: BoardState, refs: readonly string[]): Set<string> {
  const out = new Set<string>();
  const groups = board.drawings?.groups ?? [];
  for (const raw of refs) {
    const id = padParent(board, raw) ?? raw;
    out.add(id);
    // A group's items, and the groups it holds (their boxes move with it too).
    for (const g of groupAndDescendants(groups, id)) out.add(g);
    for (const m of groupLeaves(groups, id)) out.add(padParent(board, m) ?? m);
  }
  return out;
}

export interface Carried {
  /** The board without the carried items: what stays put. */
  still: BoardState;
  /** The board with nothing but the carried items: what is drawn through the transform. */
  moving: BoardState;
  /** The carried footprints' references, for the live airwires. */
  partRefs: string[];
}

/** Split the board into what stays and what is carried. */
export function splitCarried(board: BoardState, refs: readonly string[]): Carried {
  const ids = carriedIds(board, refs);
  const take = <T extends { id: string }>(list: readonly T[] | undefined): { in: T[]; out: T[] } => {
    const picked: T[] = [];
    const rest: T[] = [];
    for (const item of list ?? []) (ids.has(item.id) ? picked : rest).push(item);
    return { in: picked, out: rest };
  };
  const parts = { in: board.parts.filter((p) => p.placed && ids.has(p.ref)), out: board.parts.filter((p) => !(p.placed && ids.has(p.ref))) };
  const tracks = take(board.routing?.tracks);
  const vias = take(board.routing?.vias);
  const zones = take(board.routing?.zones);
  const shapes = take(board.drawings?.shapes);
  const texts = take(board.drawings?.texts);
  const dimensions = take(board.drawings?.dimensions);
  const groups = take(board.drawings?.groups);
  const routing = (side: "in" | "out") => (board.routing ? { ...board.routing, tracks: tracks[side], vias: vias[side], zones: zones[side] } : null);
  const drawings = (side: "in" | "out") => (board.drawings ? { ...board.drawings, shapes: shapes[side], texts: texts[side], dimensions: dimensions[side], groups: groups[side] } : null);
  return {
    still: { ...board, parts: parts.out, routing: routing("out"), drawings: drawings("out") },
    moving: { ...board, parts: parts.in, routing: routing("in"), drawings: drawings("in") },
    partRefs: parts.in.map((p) => p.ref),
  };
}
