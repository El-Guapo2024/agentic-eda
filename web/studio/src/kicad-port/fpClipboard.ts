// The Footprint Editor's Paste (`PCB_CONTROL::Paste` with `isFootprintEditor`, pcbnew/tools/pcb_control.cpp at 8303b2ad; UI-LEFT-2 FF1): which clipboard texts are
// KiCad's (`CLIPBOARD_IO::Parse` reads a footprint's `(footprint ..)` -- a `(module ..)` of KiCad 6 too -- or a board's `(kicad_pcb ..)`; their items go in through the
// `fp_item paste` verb, crates/ops/src/fp_copy.rs), and the text item any other text becomes ("If it wasn't content, then paste as a text object": the text as it is on
// the active layer, with `EDA_TEXT`'s own defaults, as the board's paste makes it -- kicad-port/plainTextPaste.ts).
//
// Pure: no React, no DOM.
import type { CmdText } from "../api/types";
import { plainTextPaste } from "./plainTextPaste";

/** `CLIPBOARD_IO::Parse` succeeds (`"kicad"`) or the paste makes a text of the clipboard (`"text"`); an empty clipboard pastes nothing (`"none"`). */
export function fpClipboardKind(text: string): "kicad" | "text" | "none" {
  if (text === "") return "none";
  return /^\s*\(\s*(footprint|module|kicad_pcb)\b/.test(text) ? "kicad" : "text";
}

/** The text item a plain-text paste adds to the footprint (`item->SetText( clipText ); item->SetLayer( GetActiveLayer() )`), and whether KiCad asks first (`longText`). */
export function fpPlainTextPaste(text: string, at: { x: number; y: number }, layer: string): { text: CmdText; longText: boolean } | null {
  const p = plainTextPaste(text, at, layer);
  if (!p || p.cmd.op !== "add_text") return null;
  return { text: { ...p.cmd.text }, longText: p.longText };
}
