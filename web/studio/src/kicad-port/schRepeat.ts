// Port of "Repeat Last Item" -- `SCH_EDIT_TOOL::RepeatDrawItem` (eeschema/tools/sch_edit_tool.cpp), action
// `eeschema.InteractiveEdit.repeatDrawItem` (Insert; F1 on macOS).
//
// The frame keeps copies of what was last placed (`SCH_EDIT_FRAME::SaveCopyForRepeatItem`, called by the
// drawing tools after a symbol, label, text, no-connect, bus entry, wire ... lands) and F1 places another
// copy of each:
//   * a label's text is incremented first (`IncrementLabel( repeat_label_increment = 1 )`: D0 -> D1);
//   * a symbol -- a power symbol is one (`SCH_SYMBOL_T`) -- is cloned *at the cursor* (`newItem->Move( cursorPos - newItem->GetPosition() )`),
//     handed to the Move tool (`RunSynchronousAction( SCH_ACTIONS::move, &commit )`: it rides the cursor until a click drops it, one undo step with
//     the copy) and, with automatic annotation on (the default), takes the next free reference (`SYMBOL_FILTER_NON_POWER`: a power symbol is
//     numbered as any new one is);
//   * everything else is moved by `default_repeat_offset_x/y` (0, 100 mil -- one grid step down);
//   * the copies become the new repeat source, so the next press steps on again (D1 -> D2).
// A junction is not repeatable (`allowRepeat` is false for it); neither is a deletion or an edit.
//
// The studio records the `Cmd`s of a successful placement (state/store.tsx's `runCmd`), so a copy is just
// the same `Cmd` with its position shifted -- one undo step per press, like the C++'s one commit.
import type { Cmd } from "../api/types";
import { incrementLabelText } from "./incrementLabelText";

/** `eeschema_settings.cpp`: `drawing.default_repeat_offset_x` 0 mil, `default_repeat_offset_y` 100 mil -- um. */
export const REPEAT_OFFSET_NM: readonly [number, number] = [0, 2_540_000];

type Repeatable =
  | Extract<Cmd, { op: "add_label" | "add_sch_text" | "add_power_symbol" | "add_no_connect" | "add_bus_entry" | "add_wire" | "add_sch_line" | "add_symbol" }>
  // a label or text put down with its dialog's properties (spin, size, "Auto", justification): the same placement, repeated with them
  | Extract<Cmd, { op: "sch_edit"; verb: "place_label" | "place_text" | "add_wire_as" }>;

const REPEATABLE_OPS = new Set(["add_label", "add_sch_text", "add_power_symbol", "add_no_connect", "add_bus_entry", "add_wire", "add_sch_line", "add_symbol"]);

/** Place Power's kind for the symbol it just placed (`set_new_power_kind`, not in the typed verbs): the copy `RepeatDrawItem` makes keeps its source's library
 *  symbol, so the repeat converts it the same way (and `SCH_SCREEN::Append` gives it the same cache entry, PARITY-sch §82). */
const isPowerKind = (c: Cmd): boolean => c.op === "sch_edit" && (c as { verb?: string }).verb === "set_new_power_kind";

const isRepeatable = (c: Cmd): c is Repeatable => REPEATABLE_OPS.has(c.op) || isPowerKind(c) || (c.op === "sch_edit" && (c.verb === "place_label" || c.verb === "place_text" || c.verb === "add_wire_as"));

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
  /** Preferences > Schematic Editor > Editing Options' Repeated Items, "Horizontal pitch:" and "Vertical pitch:" (`default_repeat_offset_x` / `_y`), nm;
   *  `REPEAT_OFFSET_NM` when absent. */
  offsetNm?: readonly [number, number];
  /** Its "Label increment:" (`repeat_label_increment`); 1 when absent. */
  labelIncrement?: number;
}

/** The commands that place the next copy of `source`. */
export function repeatCmds(source: readonly Cmd[], ctx: RepeatContext): Cmd[] {
  const [ox, oy] = ctx.offsetNm ?? REPEAT_OFFSET_NM;
  const inc = ctx.labelIncrement ?? 1;
  const shift = (p: { x: number; y: number }) => ({ x: p.x + ox, y: p.y + oy });
  const out: Cmd[] = [];
  for (const c of source) {
    if (!isRepeatable(c)) continue;
    switch (c.op) {
      case "sch_edit":
        // the dialog's properties come along; a label's text steps on
        if (isPowerKind(c)) out.push(c);
        else if (c.verb === "add_wire_as") out.push({ ...c, pts: c.pts.map(shift) });
        else out.push(c.verb === "place_label" ? { ...c, net: incrementLabelText(c.net, inc), at: shift(c.at) } : { ...c, at: shift(c.at) });
        break;
      case "add_label":
        out.push({ ...c, net: incrementLabelText(c.net, inc), at: shift(c.at) });
        break;
      case "add_power_symbol":
        // a power symbol is a symbol: at the cursor (`if( newItem->Type() == SCH_SYMBOL_T )`), not one step down
        out.push({ ...c, at: ctx.cursor ? { x: ctx.cursor[0], y: ctx.cursor[1] } : shift(c.at) });
        break;
      case "add_sch_text":
      case "add_no_connect":
      case "add_bus_entry":
        out.push({ ...c, at: shift(c.at) });
        break;
      case "add_wire":
        // `RepeatDrawItem` runs `TrimOverLappingWires`, `AddJunctionsIfNeeded` and the commit's `CleanUp`, not `finishSegments`' `TrimWire`: a repeated wire
        // across a two-pin part is not cut between its pins (r3 C3-L1)
        out.push({ op: "sch_edit", verb: "add_wire_as", pts: c.pts.map(shift), bus: c.bus ?? false, by: "repeat" });
        break;
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

/** Does the repeat bring a symbol (a power symbol is one)? `RepeatDrawItem` then hands the copy to the Move tool (`SCH_ACTIONS::move`). */
export function repeatMoves(source: readonly Cmd[]): boolean {
  return source.some((c) => c.op === "add_symbol" || c.op === "add_power_symbol");
}

/** What `RepeatDrawItem`'s `AnnotateSymbols` numbers a repeated symbol by: the source's prefix, library symbol, value and unit (the clone keeps them). */
export interface RepeatReferenceQuery {
  sourceId: string;
  prefix: string;
  lib_id: string;
  value: string;
  unit: number;
}

/**
 * The references the repeated symbols take (UI-LEFT-2 SS17): `RepeatDrawItem` clears the clone's annotation and runs `AnnotateSymbols( ANNOTATE_SELECTION, ..,
 * SYMBOL_FILTER_NON_POWER )` with the project's order, method and start number -- over every reference of the hierarchy, with the project's reference tracker, a
 * unit joining a part of the same symbol and value that has it free. That is the server's `annotate()` (`POST /api/sch/next_reference`, `ask`); the page's
 * guess (`guess`, the sheet in view and the board's parts) stands in when the server cannot answer, or when two copies of one press would take one reference
 * (`Annotate` numbers the selection's new symbols one after another). Keyed by the source symbol's id.
 */
export async function repeatReferences(
  source: readonly Cmd[],
  ask: (q: RepeatReferenceQuery) => Promise<string | null>,
  guess: (sourceId: string, taken: readonly string[]) => string,
  prefixOf: (id: string) => string
): Promise<Map<string, string>> {
  const out = new Map<string, string>();
  const taken: string[] = [];
  for (const c of source) {
    if (c.op !== "add_symbol") continue;
    const unit = c.unit ?? 1;
    let reference = await ask({ sourceId: c.id, prefix: prefixOf(c.id), lib_id: c.lib_id, value: c.value, unit });
    if (!reference || taken.includes(reference)) reference = guess(c.id, taken);
    out.set(c.id, reference);
    taken.push(reference);
  }
  return out;
}
