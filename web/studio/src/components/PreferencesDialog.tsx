// KiCad's Preferences dialog (`PAGED_DIALOG`, `EDA_BASE_FRAME::ShowPreferences`) with two of its pages: Mouse and Touchpad (below) and PCB Editor > Colors
// (components/PcbColorsPage.tsx, `PANEL_PCBNEW_COLOR_SETTINGS`), with the dialog's "Reset Colors to Defaults" on the Colors page.
//
// Port of PANEL_MOUSE_SETTINGS, the "Mouse and Touchpad" page of KiCad's Preferences dialog
// (common/dialogs/panel_mouse_settings.cpp), opened by `common.SuiteControl.openPreferences` (Ctrl+,), with the board editor's "Step for rotate commands" and
// "Flip board items" and its "Magnetic Points" of PANEL_PCBNEW_EDIT_OPTIONS (pcbnew/dialogs/panel_edit_options.cpp: Snap to Pads, Snap to Tracks and Vias, Snap
// to Graphics) below it.
// Edits `state.prefs` (kicad-port/preferences.ts), which every canvas's wheel handler and the PCB canvas's
// auto-pan loop read -- see that file's header for exactly which settings are offered and which are not.
//
// Same behaviours as the C++: the scroll-gesture grid keeps one action per column (`OnScrollRadioButton` moves a
// displaced action to the first free column), a clash that remains (two "--") shows the warning and refuses OK
// (`TransferDataFromWindow`), and "Reset to Mouse/Trackpad Defaults" sets the grid (`onMouseDefaults`,
// `onTrackpadDefaults`). On macOS the Ctrl column reads "Cmd" and Alt reads "Option", as in the C++.
import { useEffect, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { assignScrollModifier, MOUSE_SCROLL_DEFAULTS, sanitizeRotationStep, scrollModSetValid, TRACKPAD_SCROLL_DEFAULTS, type Preferences, type ScrollModifier, type ScrollRow } from "../kicad-port/preferences";
import { isMac } from "../platform";
import { usePcbColorsPage } from "./PcbColorsPage";

/** The pages of the dialog's tree (`PAGED_DIALOG`'s `wxTreebook`): Mouse and Touchpad, and PCB Editor > Colors. */
type PrefsPage = "mouse" | "pcb_colors";

/** `PAGED_DIALOG` opens on the page it was last closed on (its `g_lastPage` by title). */
let lastPage: PrefsPage = "mouse";

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
  // The step is typed as text and taken when the dialog is accepted: `m_rotationAngle.GetAngleValue()` in `TransferDataFromWindow`, which keeps the old step for a value that is nothing.
  const [rotationText, setRotationText] = useState(String(state.prefs.rotationStepDeg));
  const [page, setPageState] = useState<PrefsPage>(lastPage);
  const setPage = (p: PrefsPage) => {
    lastPage = p;
    setPageState(p);
  };
  const colors = usePcbColorsPage(open, page === "pcb_colors");

  useEffect(() => {
    if (open) {
      setDraft(state.prefs);
      setRotationText(String(state.prefs.rotationStepDeg));
      setPageState(lastPage);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_PREFERENCES_DIALOG_OPEN", open: false });
  const cancel = () => {
    colors.cancel();
    close();
  };
  const valid = scrollModSetValid(draft);
  const submit = () => {
    if (!valid) return;
    dispatch({ type: "SET_PREFERENCES", prefs: { ...draft, rotationStepDeg: sanitizeRotationStep(Number(rotationText), state.prefs.rotationStepDeg) } });
    colors.commit();
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
    <div className="dialog-backdrop" onClick={cancel}>
      <div className="dialog" style={{ width: page === "pcb_colors" ? 600 : 700, maxWidth: "95vw" }} onClick={(e) => e.stopPropagation()} data-testid="preferences-dialog">
        <div className="dialog-header">
          <span>Preferences - {page === "pcb_colors" ? "Colors" : "Mouse and Touchpad"}</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", gap: 12 }}>
          {/* The page tree (`PAGED_DIALOG`'s `wxTreebook`): "PCB Editor" is a blank parent page (`book->AddPage( new wxPanel( book ), _( "PCB Editor" ) )`). */}
          <div role="tree" aria-label="Preferences pages" style={{ width: 150, flexShrink: 0, fontSize: 12, borderRight: "1px solid var(--chrome-border)", paddingRight: 8 }}>
            {(
              [
                ["mouse", "Mouse and Touchpad", 0],
                [null, "PCB Editor", 0],
                ["pcb_colors", "Colors", 1],
              ] as const
            ).map(([id, label, depth]) => (
              <div
                key={label}
                role="treeitem"
                aria-selected={id !== null && page === id}
                style={{ padding: "3px 4px", paddingLeft: 4 + depth * 14, cursor: id ? "pointer" : "default", background: id !== null && page === id ? "var(--chrome-accent-bg, rgba(74,163,255,0.2))" : undefined, opacity: id ? 1 : 0.75 }}
                onClick={() => id && setPage(id)}
              >
                {label}
              </div>
            ))}
          </div>
          <div style={{ flex: 1, minWidth: 0 }}>
            {page === "pcb_colors" && colors.element}
            {page === "mouse" && (
              <>
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
              <div className="filter-row" title="What dragging with the middle mouse button does (schematic editor)">
                <span style={{ width: 170 }}>Middle button drag:</span>
                <select aria-label="Middle button drag" data-testid="pref-drag-middle" value={draft.dragMiddle} onChange={(e) => set({ dragMiddle: e.target.value as Preferences["dragMiddle"] })}>
                  <option value="pan">Pan</option>
                  <option value="zoom">Zoom</option>
                  <option value="none">None</option>
                </select>
              </div>
              <div className="filter-row" title="What dragging with the right mouse button does (schematic editor); a click without a drag still opens the context menu">
                <span style={{ width: 170 }}>Right button drag:</span>
                <select aria-label="Right button drag" data-testid="pref-drag-right" value={draft.dragRight} onChange={(e) => set({ dragRight: e.target.value as Preferences["dragRight"] })}>
                  <option value="pan">Pan</option>
                  <option value="zoom">Zoom</option>
                  <option value="none">None</option>
                </select>
              </div>
              <div className="filter-row" title="How fast to pan when moving an object off the edge of the screen">
                <span style={{ width: 120 }}>Auto pan speed:</span>
                <input type="range" min={1} max={10} step={1} aria-label="Auto pan speed" value={draft.autoPanAcceleration} onChange={(e) => set({ autoPanAcceleration: Number(e.target.value) })} />
                <span style={{ width: 20, textAlign: "right" }}>{draft.autoPanAcceleration}</span>
              </div>

              {heading("Editing Options (PCB Editor)")}
              <div className="filter-row" title="Set increment (in degrees) for context menu and hotkey rotation.">
                <span style={{ width: 170 }}>Step for rotate commands:</span>
                <input
                  type="number"
                  aria-label="Step for rotate commands"
                  data-testid="pref-rotation-step"
                  style={{ width: 70 }}
                  min={0.1}
                  max={359.9}
                  step={0.1}
                  value={rotationText}
                  onChange={(e) => setRotationText(e.target.value)}
                />
                <span>deg</span>
              </div>
              <div className="filter-row" title="Which way Change Side / Flip turns a board item over.">
                <span style={{ width: 170 }}>Flip board items:</span>
                <label style={{ marginRight: 12 }}>
                  <input type="radio" name="flip-direction" data-testid="pref-flip-left-right" checked={draft.flipDirection === "left_right"} onChange={() => set({ flipDirection: "left_right" })} /> Left/right
                </label>
                <label>
                  <input type="radio" name="flip-direction" data-testid="pref-flip-top-bottom" checked={draft.flipDirection === "top_bottom"} onChange={() => set({ flipDirection: "top_bottom" })} /> Top/bottom
                </label>
              </div>

              {heading("Editing Options (Schematic Editor)")}
              <label className="filter-row" title="Allow swapping any two pins of a symbol, not only the pins its library declares interchangeable.">
                <input type="checkbox" data-testid="pref-allow-pin-swaps" checked={draft.allowUnconstrainedPinSwaps} onChange={(e) => set({ allowUnconstrainedPinSwaps: e.target.checked })} />
                Allow unconstrained pin swaps
              </label>

              {heading("Magnetic Points (PCB Editor)")}
              {(
                [
                  ["Snap to Pads:", "magneticPads", "The pads the drawing and move tools snap to: never, only while a track is being routed (KiCad's default), or always."],
                  ["Snap to Tracks and Vias:", "magneticTracks", "The tracks and vias the drawing and move tools snap to: never, only while a track is being routed (KiCad's default), or always."],
                ] as const
              ).map(([label, key, tip]) => (
                <div className="filter-row" key={key} title={tip}>
                  <span style={{ width: 170 }}>{label}</span>
                  <select aria-label={label} value={draft[key]} onChange={(e) => set({ [key]: e.target.value as Preferences[typeof key] })}>
                    <option value="never">Never</option>
                    <option value="track-tool">In Track Tool</option>
                    <option value="always">Always</option>
                  </select>
                </div>
              ))}
              <label className="filter-row" title="Snap the drawing and move tools to the ends, middles, centres and corners of graphic shapes">
                <input type="checkbox" checked={draft.magneticGraphics} onChange={(e) => set({ magneticGraphics: e.target.checked })} />
                Snap to Graphics
              </label>

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
              </>
            )}
          </div>
        </div>
        <div className="dialog-footer">
          {/* `PAGED_DIALOG`'s reset button: "Reset %s to Defaults" on a page that can be reset (the Colors page's `ResetPanel`) */}
          {page === "pcb_colors" && (
            <button style={{ marginRight: "auto" }} title="Reset all colors in this theme to the KiCad defaults" onClick={() => colors.reset()}>
              Reset Colors to Defaults
            </button>
          )}
          <button onClick={cancel}>Cancel</button>
          <button className="primary" disabled={!valid} onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
