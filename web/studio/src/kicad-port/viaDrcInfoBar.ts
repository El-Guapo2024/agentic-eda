// Draw Via's DRC message as KiCad's info bar shows it (`VIA_PLACER::PlaceItem`, pcbnew/tools/drawing_tool.cpp at 8303b2ad), UI-LEFT-3 P3-32.

/** `PlaceItem`'s info bar message when `checkDRCViolation` finds one, word for word (crates/ops/src/via_place.rs `VIA_VIOLATES_DRC`). */
export const VIA_VIOLATES_DRC = "Via location violates DRC.";

/**
 * Whether a toast is `PlaceItem`'s DRC info bar (`ShowInfoBarError( _( "Via location violates DRC." ), true, WX_INFOBAR::MESSAGE_TYPE::DRC_VIOLATION )`): the
 * message of a refused via, or of one Allow DRC violations placed. KiCad's bar has a close button and stays until it is closed, another message takes its
 * place, or a clean via dismisses it (`if( infobar->GetMessageType() == DRC_VIOLATION ) infobar->Dismiss()`), UI-LEFT-3 P3-32.
 */
export function isViaDrcMessage(op: string, message: string | undefined): boolean {
  return op === "place_via" && (message ?? "").includes(VIA_VIOLATES_DRC);
}
