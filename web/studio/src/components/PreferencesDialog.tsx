// Port of PANEL_MOUSE_SETTINGS, the "Mouse and Touchpad" page of KiCad's Preferences dialog
// (common/dialogs/panel_mouse_settings.cpp), opened by `common.SuiteControl.openPreferences` (Ctrl+,).
// Edits `state.prefs` (kicad-port/preferences.ts), which every canvas's wheel handler and the PCB canvas's
// auto-pan loop read -- see that file's header for exactly which settings are offered and which are not.
//
// Same behaviours as the C++: the scroll-gesture grid keeps one action per column (`OnScrollRadioButton` moves a
// displaced action to the first free column), a clash that remains (two "--") shows the warning and refuses OK
// (`TransferDataFromWindow`), and "Reset to Mouse/Trackpad Defaults" sets the grid (`onMouseDefaults`,
// `onTrackpadDefaults`). On macOS the Ctrl column reads "Cmd" and Alt reads "Option", as in the C++.
import { useEffect, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { assignScrollModifier, MOUSE_SCROLL_DEFAULTS, scrollModSetValid, TRACKPAD_SCROLL_DEFAULTS, type Preferences, type ScrollModifier, type ScrollRow } from "../kicad-port/preferences";
import { isMac } from "../platform";

const COLUMNS: { value: ScrollModifier; label: string }[] = [
  { value: "none", label: "--" },
  { value: "ctrl", label: isMac() ? "Cmd" : "Ctrl" },
  { value: "shift", label: "Shift" },
  { value: "alt", label: isMac() ? "Option" : "Alt" },
];

const ROW_KEY: Record<ScrollRow, "scrollModifierZoom" | "scrollModifierPanH" | "scrollModifierPanV"> = { zoom: "scrollModifierZoom", panH: "scrollModifierPanH", panV: "scrollModifierPanV" };

export function PreferencesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.preferencesDialogOpen;
  const [draft, setDraft] = useState<Preferences>(state.prefs);

  useEffect(() => {
    if (open) setDraft(state.prefs);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_PREFERENCES_DIALOG_OPEN", open: false });
  const valid = scrollModSetValid(draft);
  const submit = () => {
    if (!valid) return;
    dispatch({ type: "SET_PREFERENCES", prefs: draft });
    close();
  };
  const set = (patch: Partial<Preferences>) => setDraft((d) => ({ ...d, ...patch }));

  const row = (label: string, r: ScrollRow, reverse?: { checked: boolean; onChange: (v: boolean) => void }) => (
    <tr>
      <td style={{ paddingRight: 12 }}>{label}</td>
      {COLUMNS.map((c) => (
        <td key={c.value} style={{ textAlign: "center", padding: "2px 10px" }}>
          <input type="radio" name={`scroll-${r}`} aria-label={`${label} ${c.label}`} checked={draft[ROW_KEY[r]] === c.value} onChange={() => setDraft((d) => assignScrollModifier(d, r, c.value))} />
        </td>
      ))}
      <td>
        {reverse && (
          <label className="filter-row" style={{ margin: 0 }}>
            <input type="checkbox" checked={reverse.checked} onChange={(e) => reverse.onChange(e.target.checked)} />
            Reverse
          </label>
        )}
      </td>
    </tr>
  );

  const heading = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 520 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Preferences - Mouse and Touchpad</span>
        </div>
        <div className="dialog-body">
          {heading("Pan and Zoom")}
          <label className="filter-row" title="When drawing a track or moving an item, pan when approaching the edge of the display.">
            <input type="checkbox" checked={draft.autoPan} onChange={(e) => set({ autoPan: e.target.checked })} />
            Automatically pan while moving object
          </label>
          <label className="filter-row" title="Zoom faster when scrolling quickly">
            <input type="checkbox" checked={draft.zoomAcceleration} onChange={(e) => set({ zoomAcceleration: e.target.checked })} />
            Use zoom acceleration{isMac() ? " (macOS always zooms at a constant rate, as in KiCad)" : ""}
          </label>
          <div className="filter-row" title="How far to zoom in for each rotation of the mouse wheel">
            <span style={{ width: 120 }}>Zoom speed:</span>
            <input type="range" min={1} max={10} step={1} aria-label="Zoom speed" value={draft.zoomSpeed} disabled={draft.zoomSpeedAuto} onChange={(e) => set({ zoomSpeed: Number(e.target.value) })} />
            <span style={{ width: 20, textAlign: "right" }}>{draft.zoomSpeed}</span>
            <label className="filter-row" style={{ margin: 0 }} title="Pick the zoom speed automatically">
              <input type="checkbox" checked={draft.zoomSpeedAuto} onChange={(e) => set({ zoomSpeedAuto: e.target.checked })} />
              Automatic
            </label>
          </div>
          <div className="filter-row" title="How fast to pan when moving an object off the edge of the screen">
            <span style={{ width: 120 }}>Auto pan speed:</span>
            <input type="range" min={1} max={10} step={1} aria-label="Auto pan speed" value={draft.autoPanAcceleration} onChange={(e) => set({ autoPanAcceleration: Number(e.target.value) })} />
            <span style={{ width: 20, textAlign: "right" }}>{draft.autoPanAcceleration}</span>
          </div>

          {heading("Scroll Gestures")}
          <div style={{ fontSize: 12, opacity: 0.8, marginBottom: 4 }}>Vertical touchpad or scroll wheel movement:</div>
          <table style={{ borderCollapse: "collapse" }}>
            <thead>
              <tr>
                <th />
                {COLUMNS.map((c) => (
                  <th key={c.value} style={{ fontWeight: 500, padding: "0 10px" }}>
                    {c.label}
                  </th>
                ))}
                <th />
              </tr>
            </thead>
            <tbody>
              {row("Zoom:", "zoom", { checked: draft.reverseScrollZoom, onChange: (v) => set({ reverseScrollZoom: v }) })}
              {row("Pan up/down:", "panV")}
              {row("Pan left/right:", "panH", { checked: draft.reverseScrollPanH, onChange: (v) => set({ reverseScrollPanH: v }) })}
            </tbody>
          </table>
          <div style={{ minHeight: 18, marginTop: 4, fontSize: 12, color: "#ff6666" }}>{valid ? "" : "Only one action can be assigned to each column"}</div>
          <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
            <button onClick={() => setDraft((d) => ({ ...d, ...MOUSE_SCROLL_DEFAULTS }))}>Reset to Mouse Defaults</button>
            <button onClick={() => setDraft((d) => ({ ...d, ...TRACKPAD_SCROLL_DEFAULTS }))}>Reset to Trackpad Defaults</button>
          </div>
          <div style={{ fontSize: 11, opacity: 0.65, marginTop: 10 }}>
            Not offered in the browser: center and warp cursor on zoom (a page cannot move the pointer), the drag gestures and the pan-on-movement key.
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!valid} onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
