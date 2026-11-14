// The Symbol Editor's info bar (`WX_INFOBAR` over the canvas, `EDA_BASE_FRAME::ShowInfoBarError`): what a library action refused -- "Symbol library '%s' not found.",
// "Symbol library '%s' is not writable.", "No symbol selected" (`SYMBOL_EDITOR_CONTROL::CutCopyDelete`, `DuplicateSymbol`, `AddSymbol`) -- shown with the error icon for eight
// seconds (`ShowMessageFor( aErrorMsg, 8000, wxICON_ERROR )`), then dismissed; a new message replaces the one shown and starts the time again (UI-LEFT-3 Y3-7). While it shows,
// it stands where the derived symbol's message stands (one bar per frame).
import { useEffect } from "react";
import { useSymDispatch, useSymState } from "../../state/symbolEditorStore";

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
  const icon = infoBar.kind === "error" ? "⛔" : infoBar.kind === "warning" ? "⚠" : "ℹ";
  return (
    <div
      role="alert"
      data-testid="sym-infobar"
      data-kind={infoBar.kind}
      style={{ position: "absolute", top: 0, left: 0, right: 0, zIndex: 6, padding: "4px 10px", fontSize: 12, background: "var(--chrome-bg-raised)", borderBottom: `1px solid ${infoBar.kind === "error" ? "#ef5b5b" : "var(--chrome-border)"}`, display: "flex", gap: 8, alignItems: "center" }}
    >
      <span aria-hidden>{icon}</span>
      <span data-testid="sym-infobar-text" style={{ flex: 1 }}>
        {infoBar.message}
      </span>
    </div>
  );
}
