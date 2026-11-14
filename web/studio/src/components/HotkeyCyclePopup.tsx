// `HOTKEY_CYCLE_POPUP` (common/dialogs/hotkey_cycle_popup.cpp; its look is `EDA_VIEW_SWITCHER_BASE`'s: a title over a list box) -- state/hotkeyPopup.ts holds what it
// shows. Centred on the window (`Centre()`); it takes no focus and no clicks here, so the keys pressed while it shows go on to the canvas as KiCad's `TryBefore` sends
// them (UI-LEFT-3 C3-9).
import { useHotkeyPopup } from "../state/hotkeyPopup";

export function HotkeyCyclePopup() {
  const popup = useHotkeyPopup();
  if (!popup) return null;
  return (
    <div
      role="status"
      data-testid="hotkey-cycle-popup"
      style={{ position: "fixed", left: "50%", top: "50%", transform: "translate(-50%, -50%)", zIndex: 4000, pointerEvents: "none", background: "var(--chrome-bg-raised, #2a2a2a)", border: "1px solid var(--chrome-border, #555)", borderRadius: 4, boxShadow: "0 6px 20px rgba(0,0,0,.4)", padding: "6px 8px", fontSize: 12, minWidth: 120 }}
    >
      <div data-testid="hotkey-cycle-popup-title" style={{ fontWeight: 600, marginBottom: 4 }}>
        {popup.title}
      </div>
      <div role="listbox" aria-label={popup.title} style={{ border: "1px solid var(--chrome-border, #555)", background: "var(--chrome-bg, #1e1e1e)" }}>
        {popup.items.map((item, i) => (
          <div key={`${i}-${item}`} role="option" aria-selected={i === popup.selection} style={{ padding: "2px 10px", whiteSpace: "nowrap", background: i === popup.selection ? "var(--chrome-accent-bg, rgba(74,163,255,0.35))" : undefined }}>
            {item}
          </div>
        ))}
      </div>
    </div>
  );
}
