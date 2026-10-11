// The details of the PCB edit tools that earlier sweeps left (docs/parity/GAPS.md package P6): Paste Special, and the rows of item 15 that are the board's own.
// Each handler cites the KiCad function it ports (pcbnew/ and common/ at 8303b2ad). `registerPcbEditDetailsActions` adds the handlers to useActionRunner's
// registry, after the shared and the schematic clipboard ones it may chain to.

import { createElement } from "react";
import { readClipboardText } from "../api/libraryClient";
import { snapPoint } from "../components/canvas/gridHelper";
import { addCornerCmds, cornerTarget } from "../kicad-port/pcbAddCorner";
import { clearNetsOffered, pasteOptions, pasteSource } from "../kicad-port/pcbPasteSpecial";
import { PcbPasteSpecialDialog } from "../components/PcbPasteSpecialDialog";
import { openSweepDialog } from "./pcbSweepDialogs";
import type { SweepCtx } from "./pcbSweepKit";

type Handler = () => void;

export function registerPcbEditDetailsActions(m: Map<string, Handler>, ctx: SweepCtx): void {
  const { state, api } = ctx;

  // PCB_CONTROL::Paste from ACTIONS::pasteSpecial: when the clipboard parses as a board or a footprint, DIALOG_PASTE_SPECIAL asks how the reference designators are
  // handled (and, for a board fragment, whether the nets are cleared); Cancel pastes nothing. A clipboard that is not KiCad's pastes as a text item without asking
  // ("When the clipboard doesn't parse, create a PCB item with the clipboard contents"). The schematic's Paste Special is its own (actions/schClipboardActions.ts).
  const schematicPasteSpecial = m.get("common.Interactive.pasteSpecial");
  m.set("common.Interactive.pasteSpecial", () => {
    if (state.tab !== "pcb") {
      schematicPasteSpecial?.();
      return;
    }
    void (async () => {
      const source = pasteSource(await readClipboardText(), api.getState().clipboard?.text);
      if (!source) return;
      if (source.kind === "text") {
        await api.pasteClipboard();
        return;
      }
      openSweepDialog({
        kind: "element",
        element: createElement(PcbPasteSpecialDialog, {
          showClearNets: clearNetsOffered(source),
          onOk: (mode, clearNets) => void api.pasteClipboard(pasteOptions(mode, clearNets, source)),
        }),
      });
    })();
  });

  // PCB_POINT_EDITOR::addCorner (Insert; "Create Corner" in the context menu): on the one selected zone, polygon, segment or arc, a corner at the point of its outline
  // nearest the cursor -- the cursor on the grid (`GetCursorPosition()`), where the context menu was opened when it was the menu that asked.
  const target = cornerTarget(state.board, state.selection);
  const cursor = state.pcbx.menuCursorUm ?? state.cursorUm;
  if (target && cursor) {
    m.set("pcbnew.PointEditor.addCorner", () => {
      if (state.tab !== "pcb") return;
      const [gx, gy] = snapPoint(cursor.x, cursor.y, state.gridUm);
      const cmds = addCornerCmds(target, [gx, gy]);
      if (cmds) void api.cmdBatch(cmds);
    });
  }
}
