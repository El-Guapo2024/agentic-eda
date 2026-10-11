// The details of the PCB edit tools that earlier sweeps left (docs/parity/GAPS.md package P6): Paste Special, and the rows of item 15 that are the board's own.
// Each handler cites the KiCad function it ports (pcbnew/ and common/ at 8303b2ad). `registerPcbEditDetailsActions` adds the handlers to useActionRunner's
// registry, after the shared and the schematic clipboard ones it may chain to.

import { createElement } from "react";
import { readClipboardText } from "../api/libraryClient";
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
}
