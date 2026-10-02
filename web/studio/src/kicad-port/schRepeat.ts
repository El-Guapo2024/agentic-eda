// Port of "Repeat Last Item" -- `SCH_EDIT_TOOL::RepeatDrawItem` (eeschema/tools/sch_edit_tool.cpp), action
// `eeschema.InteractiveEdit.repeatDrawItem` (Insert; F1 on macOS).
//
// The frame keeps copies of what was last placed (`SCH_EDIT_FRAME::SaveCopyForRepeatItem`, called by the
// drawing tools after a symbol, label, text, no-connect, bus entry, wire ... lands) and F1 places another
// copy of each:
//   * a label's text is incremented first (`IncrementLabel( repeat_label_increment = 1 )`: D0 -> D1);
//   * a symbol is cloned *at the cursor* (`newItem->Move( cursorPos - newItem->GetPosition() )`) and, with
//     automatic annotation on (the default), takes the next free reference;
//   * everything else is moved by `default_repeat_offset_x/y` (0, 100 mil -- one grid step down);
//   * the copies become the new repeat source, so the next press steps on again (D1 -> D2).
// A junction is not repeatable (`allowRepeat` is false for it); neither is a deletion or an edit.
//
// The studio records the `Cmd`s of a successful placement (state/store.tsx's `runCmd`), so a copy is just
// the same `Cmd` with its position shifted -- one undo step per press, like the C++'s one commit.
import type { Cmd } from "../api/types";
import { incrementLabelText } from "./incrementLabelText";

/** `eeschema_settings.cpp`: `drawing.default_repeat_offset_x` 0 mil, `default_repeat_offset_y` 100 mil -- um. */
export const REPEAT_OFFSET_UM: readonly [number, number] = [0, 2540];

type Repeatable = Extract<Cmd, { op: "add_label" | "add_sch_text" | "add_power_symbol" | "add_no_connect" | "add_bus_entry" | "add_wire" | "add_sch_line" | "add_symbol" }>;

const REPEATABLE_OPS = new Set(["add_label", "add_sch_text", "add_power_symbol", "add_no_connect", "add_bus_entry", "add_wire", "add_sch_line", "add_symbol"]);

const isRepeatable = (c: Cmd): c is Repeatable => REPEATABLE_OPS.has(c.op);

/** What a just-run command contributes to "the last placed item(s)": the command itself, or every part of a batch of placements. Null when it is not a placement (the old repeat source stays). */
export function repeatSource(cmd: Cmd): Cmd[] | null {
  if (cmd.op === "batch") {
    const parts = cmd.cmds;
    return parts.length > 0 && parts.every(isRepeatable) ? [...parts] : null;
  }
  return isRepeatable(cmd) ? [cmd] : null;
}

export interface RepeatContext {
  /** The cursor, snapped to the grid (`GetCursorPosition( true )`); a repeated symbol lands on it. */
  cursor: readonly [number, number] | null;
  /** The reference the clone takes (`AnnotateSymbols`): given the source symbol's reference, the next free one with the same prefix. */
  nextReference: (sourceId: string) => string;
}

const shift = (p: { x: number; y: number }) => ({ x: p.x + REPEAT_OFFSET_UM[0], y: p.y + REPEAT_OFFSET_UM[1] });

/** The commands that place the next copy of `source`. */
export function repeatCmds(source: readonly Cmd[], ctx: RepeatContext): Cmd[] {
  const out: Cmd[] = [];
  for (const c of source) {
    if (!isRepeatable(c)) continue;
    switch (c.op) {
      case "add_label":
        out.push({ ...c, net: incrementLabelText(c.net, 1), at: shift(c.at) });
        break;
      case "add_sch_text":
      case "add_power_symbol":
      case "add_no_connect":
      case "add_bus_entry":
        out.push({ ...c, at: shift(c.at) });
        break;
      case "add_wire":
      case "add_sch_line":
        out.push({ ...c, pts: c.pts.map(shift) });
        break;
      case "add_symbol":
        out.push({ ...c, id: ctx.nextReference(c.id), at: ctx.cursor ? { x: ctx.cursor[0], y: ctx.cursor[1] } : shift(c.at) });
        break;
    }
  }
  return out;
}
