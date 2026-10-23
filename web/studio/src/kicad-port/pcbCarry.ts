// The picture of a selection being carried on the PCB canvas: which items are in the hand, and the one transform that shows them where the drop
// would put them.
//
// `EDIT_TOOL::doMoveSelection` moves the real items under the cursor and `Rotate` / `Flip` act on them in place, about the selection's reference
// point, which travels with the cursor. Nothing is committed until the drop, so this app keeps the items where they are and draws them through
// a transform instead: the same turn and flip about where the selection was picked up, then the move -- exactly the batch `planCarry`
// (kicad-port/pcbTransform.ts) sends on the drop, so what is drawn is what lands.
//
// Pure: no store, no DOM. Unit-tested in pcbCarry.test.ts.

import type { BoardState, FieldInfo, Part } from "../api/types";
import { carryAngleDeg, type FlipDirection } from "./pcbTransform";
import { padParent } from "./pcbItems";
import { groupAndDescendants, groupLeaves } from "./groupTree";

export type Matrix = readonly [number, number, number, number, number, number];

/** What the painter needs of a `MovePreview` (state/store.tsx) to carry a PCB selection. */
export interface CarryPreview {
  refs: readonly string[];
  dxUm: number;
  dyUm: number;
  /**
   * R presses (Shift+R takes one away); each turns the carried items counter-clockwise by `rotateStepDeg`, the rotation step of the preferences
   * (`PCBNEW_SETTINGS::m_RotationAngle`, 90 degrees by default -- hence the name: a quarter turn each).
   */
  rotateQuarterTurns?: number;
  rotateStepDeg?: number;
  flipped?: boolean;
  /** Which axis a flip mirrors across (`PCBNEW_SETTINGS::m_FlipDirection`); left/right when left out. */
  flipDirection?: FlipDirection;
  /** Where the selection was picked up: the point a turn is about, and the point a flip mirrors about. */
  pivotUm?: readonly [number, number];
  flipPivotUm?: readonly [number, number];
}

const IDENTITY: Matrix = [1, 0, 0, 1, 0, 0];

/** `a` applied after `b`, in the canvas order `[a, b, c, d, e, f]` (`x' = a x + c y + e`, `y' = b x + d y + f`). */
function after(a: Matrix, b: Matrix): Matrix {
  return [a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1], a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3], a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5]];
}

/**
 * `degCcw` degrees counter-clockwise on the screen about `(px, py)` (the canvas' `rotate` runs clockwise on a Y-down screen, hence the sign). A multiple of a
 * quarter turn has its exact cosine and sine, so the usual 90 degree step draws exactly where the verb puts the items.
 */
function turnMatrix(degCcw: number, px: number, py: number): Matrix {
  const quarter = degCcw / 90;
  const [cos, sin] = Number.isInteger(quarter)
    ? ((q) => (q === 0 ? [1, 0] : q === 1 ? [0, -1] : q === 2 ? [-1, 0] : [0, 1]))((((quarter % 4) + 4) % 4) as 0 | 1 | 2 | 3)
    : [Math.cos((-degCcw * Math.PI) / 180), Math.sin((-degCcw * Math.PI) / 180)];
  return [cos!, sin!, -sin!, cos!, px - cos! * px + sin! * py, py - sin! * px - cos! * py];
}

/** The mirror about the vertical line through `fx` (`FLIP_DIRECTION::LEFT_RIGHT`) or the horizontal one through `fy` (`TOP_BOTTOM`). */
function flipMatrix(fx: number, fy: number, direction: FlipDirection): Matrix {
  return direction === "top_bottom" ? [1, 0, 0, -1, 0, 2 * fy] : [-1, 0, 0, 1, 2 * fx, 0];
}

/** The transform from where the carried items are to where they are drawn: turned, then flipped, then moved. */
export function carryMatrix(p: CarryPreview): Matrix {
  let m: Matrix = IDENTITY;
  const turned = carryAngleDeg(p.rotateQuarterTurns ?? 0, p.rotateStepDeg ?? 90);
  if (turned !== 0) {
    const [px, py] = p.pivotUm ?? [0, 0];
    m = after(turnMatrix(turned, px, py), m);
  }
  if (p.flipped) {
    const [fx, fy] = p.flipPivotUm ?? [0, 0];
    m = after(flipMatrix(fx, fy, p.flipDirection ?? "left_right"), m);
  }
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
  /** Fields carried on their own (`REF:Reference` selected without its footprint): drawn through the transform, while their footprint stays. */
  fields: FieldInfo[];
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
  // A field in the hand without its footprint leaves its footprint where it is and takes only itself.
  const fields: FieldInfo[] = [];
  const stay = (p: Part): Part => {
    if (!p.fields?.some((f) => ids.has(f.id))) return p;
    fields.push(...p.fields.filter((f) => ids.has(f.id)));
    return { ...p, fields: p.fields.filter((f) => !ids.has(f.id)) };
  };
  parts.out = parts.out.map(stay);
  const tracks = take(board.routing?.tracks);
  const vias = take(board.routing?.vias);
  const zones = take(board.routing?.zones);
  const shapes = take(board.drawings?.shapes);
  const texts = take(board.drawings?.texts);
  const dimensions = take(board.drawings?.dimensions);
  const textBoxes = take(board.drawings?.text_boxes);
  const images = take(board.drawings?.images);
  const points = take(board.drawings?.points);
  const barcodes = take(board.drawings?.barcodes);
  const gridItems = take(board.drawings?.grid_items);
  const groups = take(board.drawings?.groups);
  const tables = take(board.drawings?.tables);
  const routing = (side: "in" | "out") => (board.routing ? { ...board.routing, tracks: tracks[side], vias: vias[side], zones: zones[side] } : null);
  const drawings = (side: "in" | "out") => (board.drawings ? { ...board.drawings, shapes: shapes[side], texts: texts[side], dimensions: dimensions[side], text_boxes: textBoxes[side], images: images[side], points: points[side], barcodes: barcodes[side], grid_items: gridItems[side], groups: groups[side], ...(board.drawings.tables ? { tables: tables[side] } : {}) } : null);
  return {
    still: { ...board, parts: parts.out, routing: routing("out"), drawings: drawings("out") },
    moving: { ...board, parts: parts.in, routing: routing("in"), drawings: drawings("in") },
    partRefs: parts.in.map((p) => p.ref),
    fields,
  };
}
