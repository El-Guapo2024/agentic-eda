// Left / Center / Right Justify (`ACTIONS::leftJustify` / `centerJustify` / `rightJustify`) as the `/api/cmd` verbs that carry them out.
//
//   pcbnew/tools/edit_tool.cpp   EDIT_TOOL::JustifyText: for each selected PCB_FIELD, PCB_TEXT and PCB_TEXTBOX `SetHorizJustify( LEFT / CENTER /
//                                RIGHT )`, locked items filtered out of the request (`FilterCollectorForLockedItems`), one commit (titled
//                                "Left Justify" / "Center Justify" / "Right Justify"). The same tool runs in the Footprint Editor.
//
// The board's own text is `drawings.texts` (a footprint's reference and value are not items of their own here); a footprint's texts in the
// Footprint Editor are its `texts`. One batch is one undo step, like the commit. (The Schematic Editor's `SCH_EDIT_TOOL::JustifyText` needs
// a justification on the schematic's text, which the model does not have yet.)
import type { BoardState, Cmd, CmdText, TextJustify } from "../api/types";

/**
 * The verbs that justify the board texts among `ids`: only a text whose justification changes needs one, and a locked text is skipped
 * (`skippedLocked` says how many were).
 */
export function boardJustifyCmds(board: BoardState, ids: readonly string[], justify: TextJustify): { cmds: Cmd[]; skippedLocked: number } {
  const locked = new Set(board.locked ?? []);
  const wanted = new Set(ids);
  const cmds: Cmd[] = [];
  let skippedLocked = 0;
  for (const t of board.drawings?.texts ?? []) {
    if (!wanted.has(t.id)) continue;
    if (locked.has(t.id)) {
      skippedLocked++;
      continue;
    }
    if (t.justify === justify) continue;
    cmds.push({ op: "edit_text", id: t.id, content: t.content, angle: t.angle, layer: t.layer, size_um: t.size, stroke_width: t.stroke_width, justify, mirror: t.mirror });
  }
  return { cmds, skippedLocked };
}

/** The same for the texts of the footprint open in the Footprint Editor. */
export function footprintJustifyCmds(footprint: string, texts: readonly CmdText[], ids: readonly string[], justify: TextJustify): Cmd[] {
  const wanted = new Set(ids);
  const cmds: Cmd[] = [];
  for (const t of texts) {
    if (!t.id || !wanted.has(t.id) || t.justify === justify) continue;
    cmds.push({ op: "edit_footprint_text", footprint, id: t.id, content: t.content, angle: t.angle, layer: t.layer, size_um: t.size_um, stroke_width: t.stroke_width, justify, mirror: t.mirror });
  }
  return cmds;
}
