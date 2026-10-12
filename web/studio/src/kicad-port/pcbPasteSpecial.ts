// Paste Special on the PCB: the choices of `DIALOG_PASTE_SPECIAL` and which of them a clipboard gets.
//
//   common/dialogs/dialog_paste_special.cpp        the dialog: tooltips, the mode it opens on, `HideClearNets`
//   common/dialogs/dialog_paste_special_base.cpp   the three choices of "Reference Designators" and the "Clear net assignments" box
//   pcbnew/tools/pcb_control.cpp  PCB_CONTROL::Paste   `PASTE_MODE mode = PASTE_MODE::KEEP_ANNOTATIONS; const wxString defaultRef = wxT( "REF**" );`,
//                                                      `if( clipItem->Type() != PCB_T ) dlg.HideClearNets();`
//
// Pure: no store, no DOM. The dialog is components/PcbPasteSpecialDialog.tsx; the paste itself is `Cmd::PasteSpecial` (crates/ops/src/pcb_paste_special.rs).

import type { PcbPasteMode } from "../api/types";

/** `PASTE_MODE mode = PASTE_MODE::KEEP_ANNOTATIONS;`: what a plain Paste does and the choice the dialog opens on (`TransferDataToWindow`). */
export const DEFAULT_PASTE_MODE: PcbPasteMode = "keep";

/** `const wxString defaultRef = wxT( "REF**" );` */
export const DEFAULT_REFERENCE = "REF**";

/** The choices of the "Reference Designators" box (`dialog_paste_special_base.cpp`), in the dialog's order, with the tooltips `DIALOG_PASTE_SPECIAL`'s constructor sets. */
export const PCB_PASTE_SPECIAL_OPTIONS: ReadonlyArray<{ mode: PcbPasteMode; label: string; tip: string }> = [
  { mode: "unique", label: "Assign unique reference designators to pasted symbols", tip: "Finds the next available reference designator for any designators that already exist in the design." },
  { mode: "keep", label: "Keep existing reference designators, even if they are duplicated", tip: "" },
  { mode: "remove", label: "Clear reference designators on all pasted symbols", tip: `Replaces reference designators with '${DEFAULT_REFERENCE}'.` },
];

/** The "Clear net assignments" box: its label and tooltip. */
export const CLEAR_NETS_LABEL = "Clear net assignments";
export const CLEAR_NETS_TIP = "Remove the net information from all connected items before pasting";

/** What a Paste has to paste (`PCB_CONTROL::Paste`): a board fragment or a single footprint of KiCad's clipboard text, or plain text that becomes a text item. */
export type PasteSource = { kind: "board" | "footprint" | "text"; text: string };

/** Whether `text` is a whole board (`(kicad_pcb ...)`, `clipItem->Type() == PCB_T`) rather than a single footprint. */
export function isBoardFragment(text: string): boolean {
  return text.trimStart().startsWith("(kicad_pcb");
}

const isKicadText = (t: string) => {
  const head = t.trimStart();
  return head.startsWith("(kicad_pcb") || head.startsWith("(footprint") || head.startsWith("(module");
};

/**
 * What a Paste pastes, in the order the studio looks: the system clipboard when it holds KiCad's text (a copy made in KiCad or on another board), else plain text
 * there (a text item), else this session's own copy where the browser would not hand the system clipboard over.
 */
export function pasteSource(system: string | null | undefined, session: string | null | undefined): PasteSource | null {
  if (system != null && isKicadText(system)) return { kind: isBoardFragment(system) ? "board" : "footprint", text: system };
  if (system != null && system.trim() !== "") return { kind: "text", text: system };
  if (session != null && isKicadText(session)) return { kind: isBoardFragment(session) ? "board" : "footprint", text: session };
  return null;
}

/** `if( clipItem->Type() != PCB_T ) dlg.HideClearNets();`: the box is offered for a board fragment only (a lone footprint's pads are on no net anyway). */
export function clearNetsOffered(source: PasteSource): boolean {
  return source.kind === "board";
}

/** What the dialog answers with, as the options of `pasteClipboard`. */
export function pasteOptions(mode: PcbPasteMode, clearNets: boolean, source: PasteSource): { mode: PcbPasteMode; clearNets: boolean } {
  return { mode, clearNets: clearNets && clearNetsOffered(source) };
}
