// The fill of a board shape: `FILL_T` of include/eda_shape.h and the five choices of `UI_FILL_MODE` the Properties panel and the shape dialog offer
// (None, Solid, Hatch, Reverse Hatch, Cross-hatch; `EDA_SHAPE::GetFillModeProp` / `SetFillModeProp`).
//
// The state describes a hatched shape (GAPS.md item 24, P16i step 3): `fill` is the hatch kind, `filled` is false, and `hatch_lines` are the lines
// `EDA_SHAPE::UpdateHatching` makes -- the server makes them from the board as it is, every time it describes it, which is what KiCad's
// `rehatchShapes` action, posted after every commit, undo and file load, comes to.
import type { ShapeFill } from "../api/pcbItemTypes";
import type { BoardState, Cmd, CmdShape, Shape } from "../api/types";

export type { ShapeFill };
export type HatchFill = Exclude<ShapeFill, "none" | "solid">;

/** The choices of the fill control, in `UI_FILL_MODE` order (the labels of dialog_shape_properties_base.cpp). */
export const SHAPE_FILL_CHOICES: { label: string; value: ShapeFill }[] = [
  { label: "None", value: "none" },
  { label: "Solid", value: "solid" },
  { label: "Hatch", value: "hatch" },
  { label: "Reverse Hatch", value: "reverse_hatch" },
  { label: "Cross-hatch", value: "cross_hatch" },
];

type Fillable = { filled: boolean; fill?: HatchFill };

/** `EDA_SHAPE::GetFillModeProp`. */
export function fillOf(s: Fillable): ShapeFill {
  return s.filled ? "solid" : (s.fill ?? "none");
}

/** `EDA_SHAPE::IsHatchedFill`. */
export function isHatched(s: Fillable): boolean {
  return !s.filled && s.fill !== undefined;
}

/** `EDA_SHAPE::IsAnyFill`: what takes the Filled Shapes opacity and goes with it (`PCB_SELECTION_TOOL::Selectable`). */
export function isAnyFill(s: Fillable): boolean {
  return s.filled || s.fill !== undefined;
}

/** The kinds `fillAvailable` lets have a fill: a line, an arc and a curve have none. */
export function canFill(kind: Shape["kind"]): boolean {
  return kind === "rect" || kind === "circle" || kind === "polygon";
}

function distToSegment(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax;
  const dy = by - ay;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / len2));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

/** How far `(px, py)` is from the ink of the hatching (`GetHatching().Collide`: a line's polygon is as wide as the shape's line); `Infinity` for a shape without hatch lines. */
export function hatchDistance(s: Shape, px: number, py: number): number {
  const lines = s.hatch_lines;
  if (!lines || lines.length === 0) return Infinity;
  let best = Infinity;
  for (const [x1, y1, x2, y2] of lines) best = Math.min(best, distToSegment(px, py, x1, y1, x2, y2));
  return Math.max(0, best - (s.hatch_width ?? s.stroke_width) / 2);
}

/** KiCad's message for a fill other than solid on a shape with no line width (dialog_shape_properties.cpp, Validate). */
export function fillWidthError(kind: Shape["kind"], fill: ShapeFill, widthNm: number): string | null {
  if (fill === "solid" || widthNm > 0) return null;
  switch (kind) {
    case "circle":
      return "Line width must be greater than zero for an unfilled circle.";
    case "rect":
      return "Line width must be greater than zero for an unfilled rectangle.";
    case "polygon":
      return "Line width must be greater than zero for an unfilled polygon.";
    default:
      return null;
  }
}

/**
 * The commands that make the board's shape `id` the shape `next`. A shape's id comes from its geometry, so the board editor has always done an edit of the geometry as a delete and an
 * add (the new shape takes the place of the old one in the selection: `removed` are the ids that are gone). A hatched shape, and one with line endings or a net, is replaced in place
 * instead (`replace_shape` keeps its id, and the hatch, the endings and the net are kept by id), as KiCad edits the same item; nothing is removed then.
 */
export function reshapeCmds(board: Pick<BoardState, "drawings"> | null | undefined, id: string, next: CmdShape): { cmds: Cmd[]; removed: string[] } {
  const current = board?.drawings?.shapes.find((s) => s.id === id);
  // A shape with line endings or a net keeps them the same way: they are kept by its id too.
  if (current && (isHatched(current) || current.start_ending || current.end_ending || current.net)) return { cmds: [{ op: "replace_shape", id, shape: next }], removed: [] };
  return { cmds: [{ op: "delete_shape", id }, { op: "add_shape", shape: next }], removed: [id] };
}

/**
 * The commands of a Fillet, Chamfer, Dogbone, Extend Lines or Heal Shapes result (`pcbModify.ts`'s `ModifyResult`). `EDIT_TOOL::ModifyLines` and `EDIT_TOOL::HealShapes` edit the lines
 * they keep in place (`SetStart` / `SetEnd` on the same `PCB_SHAPE`), so a kept line's endings and net survive; a modified line that has any is therefore replaced in place
 * (`reshapeCmds`), the others are deleted and added as before. The deletes come first, then the replacements, then the new shapes.
 */
export function modifyResultCmds(
  board: Pick<BoardState, "drawings"> | null | undefined,
  res: { remove: readonly string[]; add: readonly CmdShape[]; replaced: readonly { id: string; addIndex: number }[] }
): { cmds: Cmd[]; removed: string[] } {
  const byId = new Map(res.replaced.map((r) => [r.id, r.addIndex]));
  const deletes: Cmd[] = [];
  const replaces: Cmd[] = [];
  const removed: string[] = [];
  const used = new Set<number>();
  for (const id of res.remove) {
    const idx = byId.get(id);
    const next = idx === undefined ? undefined : res.add[idx];
    if (idx !== undefined && next) {
      const re = reshapeCmds(board, id, next);
      if (re.removed.length === 0) {
        replaces.push(...re.cmds);
        used.add(idx);
        continue;
      }
    }
    deletes.push({ op: "delete_shape", id });
    removed.push(id);
  }
  const adds: Cmd[] = res.add.flatMap((shape, i): Cmd[] => (used.has(i) ? [] : [{ op: "add_shape", shape }]));
  return { cmds: [...deletes, ...replaces, ...adds], removed };
}
