// `PCB_CONTROL::Paste` (pcbnew/tools/pcb_control.cpp) for clipboard text that is not KiCad's: "If it wasn't content, then paste as a text object" --
// `item->SetText( clipText ); item->SetLayer( m_frame->GetActiveLayer() );`, the text exactly as it is (not trimmed: a trailing newline is a second line), with
// `EDA_TEXT`'s own defaults (`Mils2IU( DEFAULT_SIZE_TEXT )`, 50 mils = 1.27 mm; `TEXT_ATTRIBUTES::m_StrokeWidth( 0 )`, the automatic pen, which draws at
// `GetPenSizeForNormal( size ) = KiROUND( size / 8.0 )`), and no `StyleFromSettings`. Text longer than `m_MaxPastedTextLength` (100) asks first.
import type { Cmd } from "../api/types";

/** `ADVANCED_CFG::m_MaxPastedTextLength`. */
export const MAX_PASTED_TEXT_LENGTH = 100;
/** `PCB_CONTROL::Paste`'s question before a long text. */
export const LONG_TEXT_QUESTION = "Pasting a long text text string may be very slow.  Do you want to continue?";
/** `DEFAULT_SIZE_TEXT` (50 mils), nm. */
const DEFAULT_TEXT_SIZE_NM = 1_270_000;

/** The text a plain-text paste puts on the board, and whether KiCad asks first (`longText`). `null` for an empty clipboard. Our text keeps the pen KiCad draws
 * an automatic one with (the model's text has a pen of its own), so the file says `(thickness 0.15875)` where KiCad writes none. */
export function plainTextPaste(clipText: string, at: { x: number; y: number }, layer: string): { cmd: Cmd; longText: boolean } | null {
  if (clipText === "") return null;
  const cmd: Cmd = { op: "add_text", text: { content: clipText, at, angle: 0, layer, size_nm: DEFAULT_TEXT_SIZE_NM, stroke_width: Math.round(DEFAULT_TEXT_SIZE_NM / 8), justify: "center", mirror: false } };
  return { cmd, longText: clipText.length > MAX_PASTED_TEXT_LENGTH };
}
