// Break and Slice on the sheet (kicad-port/schBreak.ts): reading the selected wires, buses and graphic lines, the live preview while the new end
// follows the cursor, and the commit when it is dropped.
import type { Dispatch } from "react";
import type { Cmd, Schematic } from "../../api/types";
import type { Action, StudioApi } from "../../state/store";
import { breakCmds, cutPieces, startBreak, type BreakMode, type BreakSource, type BreakState, type P } from "../../kicad-port/schBreak";

/** The wires, buses and graphic lines among `ids` -- what `linesSelection` lets Break and Slice work on. */
export function breakSources(sch: Schematic, ids: readonly string[]): BreakSource[] {
  const out: BreakSource[] = [];
  for (const id of ids) {
    const w = sch.wires.find((x) => x.id === id);
    if (w) {
      out.push({ id, kind: "wire", pts: w.pts, bus: w.bus });
      continue;
    }
    const l = (sch.lines ?? []).find((x) => x.id === id);
    if (l) out.push({ id, kind: "line", pts: l.pts, widthUm: l.width_um });
  }
  return out;
}

/** Start Break or Slice on the selection with the cursor at `cursor` (null when no wire or line is selected). */
export function beginBreak(sch: Schematic, ids: readonly string[], mode: BreakMode, cursor: P): BreakState | null {
  return startBreak(mode, breakSources(sch, ids), cursor);
}

/** The sheet as it would be with the cut lines replaced by their pieces at the cursor -- the live preview. */
export function breakPreviewSheet(sch: Schematic, brk: BreakState, cursor: P): Schematic {
  const cut = new Map(brk.cuts.map((c) => [c.source.id, c]));
  const wires: Schematic["wires"] = [];
  for (const w of sch.wires) {
    const c = cut.get(w.id);
    if (!c) {
      wires.push(w);
      continue;
    }
    cutPieces(brk, c, cursor).forEach((pts, i) => wires.push({ ...w, id: `${w.id}~${i}`, pts: pts.map(([x, y]) => [x, y] as [number, number]) }));
  }
  const lines: NonNullable<Schematic["lines"]> = [];
  for (const l of sch.lines ?? []) {
    const c = cut.get(l.id);
    if (!c) {
      lines.push(l);
      continue;
    }
    cutPieces(brk, c, cursor).forEach((pts, i) => lines.push({ ...l, id: `${l.id}~${i}`, pts: pts.map(([x, y]) => [x, y] as [number, number]) }));
  }
  return { ...sch, wires, lines };
}

/** Drop the new end at `cursor`: the lines are replaced by their pieces as one undo step, and the tool ends. */
export function commitBreak(brk: BreakState, cursor: P, dispatch: Dispatch<Action>, api: StudioApi): void {
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  const cmds = breakCmds(brk, cursor) as Cmd[];
  void api.cmdBatch(cmds).then((ok) => {
    if (ok) dispatch({ type: "SET_SELECTION", refs: [] });
  });
}
