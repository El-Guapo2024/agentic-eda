// Draw Via's DRC message as KiCad's info bar shows it (`VIA_PLACER::PlaceItem`, pcbnew/tools/drawing_tool.cpp at 8303b2ad), UI-LEFT-3 P3-32: the board
// editor's info bar over the canvas (components/PcbInfoBar.tsx, `state.pcbInfoBar`).

/** `PlaceItem`'s info bar message when `checkDRCViolation` finds one, word for word (crates/ops/src/via_place.rs `VIA_VIOLATES_DRC`). */
export const VIA_VIOLATES_DRC = "Via location violates DRC.";

/**
 * Whether a toast is `PlaceItem`'s DRC info bar (`ShowInfoBarError( _( "Via location violates DRC." ), true, WX_INFOBAR::MESSAGE_TYPE::DRC_VIOLATION )`): the
 * message of a refused via, or of one Allow DRC violations placed. KiCad's bar has a close button and stays until it is closed, another message takes its
 * place, or a clean via dismisses it (`if( infobar->GetMessageType() == DRC_VIOLATION ) infobar->Dismiss()`), UI-LEFT-3 P3-32.
 */
export function isViaDrcMessage(op: string, message: string | undefined): boolean {
  // Place Microvia Stack's own (`placeStack`: `ShowInfoBarError( _( "Microvia stack location violates DRC." ), true, DRC_VIOLATION )`, UI-LEFT-3 X3-1)
  return (op === "place_via" && (message ?? "").includes(VIA_VIOLATES_DRC)) || (op === "pcb_item" && (message ?? "").includes(STACK_VIOLATES_DRC));
}

/** Place Microvia Stack's info bar message, word for word (crates/ops/src/via_stack.rs `STACK_VIOLATES_DRC`). */
export const STACK_VIOLATES_DRC = "Microvia stack location violates DRC.";

/** What the info bar says: KiCad's words alone (`ShowInfoBarError( _( "Via location violates DRC." ), .. )`), without the server's error code before them. */
export function viaDrcInfoBarText(message: string): string {
  return message.includes(VIA_VIOLATES_DRC) ? VIA_VIOLATES_DRC : message.includes(STACK_VIOLATES_DRC) ? STACK_VIOLATES_DRC : message;
}

/** A placement whose clean result dismisses a standing DRC bar: a Draw Via via, or a Place Microvia Stack stack (a `set_via_stack` with no id). */
export function isViaPlacement(cmd: { op: string; verb?: string; id?: unknown }): boolean {
  return cmd.op === "place_via" || (cmd.op === "pcb_item" && cmd.verb === "set_via_stack" && cmd.id === undefined);
}
