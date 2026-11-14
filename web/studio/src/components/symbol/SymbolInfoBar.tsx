// The Symbol Editor's info bar (`WX_INFOBAR` over the canvas, `EDA_BASE_FRAME::ShowInfoBarError`): what a library action refused -- "Symbol library '%s' not found.",
// "Symbol library '%s' is not writable.", "No symbol selected" (`SYMBOL_EDITOR_CONTROL::CutCopyDelete`, `DuplicateSymbol`, `AddSymbol`) -- shown with the error icon for eight
// seconds (`ShowMessageFor( aErrorMsg, 8000, wxICON_ERROR )`), then dismissed; a new message replaces the one shown and starts the time again (UI-LEFT-3 Y3-7). While it shows,
// it stands where the derived symbol's message stands (one bar per frame). The bar itself is components/InfoBar.tsx.
import { useEffect } from "react";
import { useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { InfoBar } from "../InfoBar";

/** `ShowInfoBarError`'s time: `ShowMessageFor( aErrorMsg, 8000, ... )`. */
export const INFOBAR_ERROR_MS = 8000;

export function SymbolInfoBar() {
  const { infoBar } = useSymState();
  const dispatch = useSymDispatch();
  useEffect(() => {
    if (!infoBar) return;
    const t = setTimeout(() => dispatch({ type: "DISMISS_INFOBAR" }), INFOBAR_ERROR_MS);
    return () => clearTimeout(t);
  }, [infoBar, dispatch]);
  if (!infoBar) return null;
  return <InfoBar message={infoBar.message} kind={infoBar.kind} testid="sym-infobar" />;
}
