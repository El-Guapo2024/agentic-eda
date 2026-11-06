// KiCad's Preferences dialog (`PAGED_DIALOG`, `EDA_BASE_FRAME::ShowPreferences`) with these of its pages: Mouse and Touchpad (below), each editor's Display Options --
// the Grid Display and Cursor part of it (`PANEL_GAL_OPTIONS`, common/dialogs/panel_gal_options.cpp: Dots / Lines / Small crosses, "Grid thickness:" 0.5 to 10 pixels,
// "Minimum grid spacing:" 5 to 200 pixels, the three crosshairs and "Always show crosshairs"; each editor keeps its own, state/commonOptions.ts `window`) -- PCB
// Editor > Colors (components/PcbColorsPage.tsx, `PANEL_PCBNEW_COLOR_SETTINGS`), with the dialog's "Reset Colors to Defaults" on the Colors page, and 3D Viewer >
// General and > Realtime Rendering (3d-viewer/dialogs/panel_3D_display_options.cpp, panel_3D_opengl_options.cpp: the settings the studio's 3D view honours,
// kicad-port/viewer3dSettings.ts; the two pages at the end of this file). Not here of Display Options: "Snap to grid:" (Always / When grid shown / Never; the
// snapping always uses the grid) and the editors' other display settings.
//
// Port of PANEL_MOUSE_SETTINGS, the "Mouse and Touchpad" page of KiCad's Preferences dialog
// (common/dialogs/panel_mouse_settings.cpp), opened by `common.SuiteControl.openPreferences` (Ctrl+,), with the board editor's "Step for rotate commands" and
// "Flip board items" and its "Magnetic Points" of PANEL_PCBNEW_EDIT_OPTIONS (pcbnew/dialogs/panel_edit_options.cpp: Snap to Pads, Snap to Tracks and Vias, Snap
// to Graphics) below it.
// Edits `state.prefs` (kicad-port/preferences.ts), which every canvas's wheel, drag and auto-pan handling reads -- see that file's header for exactly which
// settings are offered and which are not.
//
// Same behaviours as the C++: the scroll-gesture grid keeps one action per column (`OnScrollRadioButton` moves a
// displaced action to the first free column), a clash that remains (two "--") shows the warning and refuses OK
// (`TransferDataFromWindow`), and "Reset to Mouse/Trackpad Defaults" sets the grid (`onMouseDefaults`,
// `onTrackpadDefaults`). On macOS the Ctrl column reads "Cmd" and Alt reads "Option", as in the C++.
import { useEffect, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { assignScrollModifier, MOUSE_SCROLL_DEFAULTS, sanitizePinStepMils, sanitizePreferences, sanitizeRotationStep, scrollModSetValid, TRACKPAD_SCROLL_DEFAULTS, type Preferences, type ScrollModifier, type ScrollRow } from "../kicad-port/preferences";
import { isMac } from "../platform";
import { usePcbColorsPage } from "./PcbColorsPage";
import { FP_PREFS_TITLES, FP_PREFS_SUBTREE, FpPreferencesPage, fpGraphicsProblem, type FpPrefsExtra, type FpPrefsPage } from "./FpPreferencesPages";
import { useFpDispatch, useFpState } from "../state/footprintEditorStore";
import { FACTORY_TEXT_GRAPHICS } from "../kicad-port/layerDefaults";
import { EDITOR_WINDOWS, getCommonOptions, setWindowOptions, type EditorWindow, type WindowOptions } from "../state/commonOptions";
import { GRID_MIN_SPACING_RANGE, GRID_THICKNESS_CHOICES, gridMinSpacingValue } from "../kicad-port/grid";
import { getViewer3DUserSettings, setViewer3DPrefs } from "../state/viewer3dUserSettings";
import { DEFAULT_VIEWER3D_PREFS, sanitizeViewer3DPrefs, type AntialiasingMode, type Viewer3DPrefs } from "../kicad-port/viewer3dSettings";

/** The pages of the dialog's tree (`PAGED_DIALOG`'s `wxTreebook`): Mouse and Touchpad, each editor's Display Options, PCB Editor > Colors, 3D Viewer > General
 *  and > Realtime Rendering. */
type PrefsPage = "mouse" | "pcb_colors" | `display_${EditorWindow}` | "3d_general" | "3d_opengl" | FpPrefsPage;

const EDITOR_PAGE_LABEL: Record<EditorWindow, string> = { pcb: "PCB Editor", schematic: "Schematic Editor", footprint: "Footprint Editor", symbol: "Symbol Editor" };

/** Each page's title in the dialog's caption. */
function pageTitle(page: PrefsPage): string {
  if (page.startsWith("display_")) return "Display Options";
  if (isFpPage(page)) return FP_PREFS_TITLES[page];
  return ({ mouse: "Mouse and Touchpad", pcb_colors: "Colors", "3d_general": "General", "3d_opengl": "Realtime Rendering" } as Record<string, string>)[page] ?? page;
}

const isFpPage = (p: string): p is FpPrefsPage => p.startsWith("fp_");

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
  // The Footprint Editor's pages (components/FpPreferencesPages.tsx): its rotation step as typed, and its line mode, which they edit beside the preferences.
  const fpState = useFpState();
  const fpDispatch = useFpDispatch();
  const [fpRotationText, setFpRotationText] = useState(String(state.prefs.fpRotationStepDeg));
  const [fpExtra, setFpExtraState] = useState<FpPrefsExtra>({ lineMode: fpState.lineMode });
  const setFpExtra = (patch: Partial<FpPrefsExtra>) => setFpExtraState((e) => ({ ...e, ...patch }));
  // each editor's Display Options as edited so far (`PANEL_GAL_OPTIONS` of its frame); OK keeps them
  const [windows, setWindows] = useState<Record<EditorWindow, WindowOptions>>(getCommonOptions().window);
  /** The 3D viewer's settings being edited (state/viewer3dUserSettings.ts), taken on OK as the other pages are. */
  const [draft3d, setDraft3d] = useState<Viewer3DPrefs>(getViewer3DUserSettings().prefs);

  useEffect(() => {
    if (open) {
      setDraft(state.prefs);
      setRotationText(String(state.prefs.rotationStepDeg));
      setFpRotationText(String(state.prefs.fpRotationStepDeg));
      setFpExtraState({ lineMode: fpState.lineMode });
      setPageState(lastPage);
      setWindows(getCommonOptions().window);
      setDraft3d(getViewer3DUserSettings().prefs);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_PREFERENCES_DIALOG_OPEN", open: false });
  const cancel = () => {
    colors.cancel();
    close();
  };
  const graphicsProblem = fpGraphicsProblem(draft.fpTextGraphics);
  const valid = scrollModSetValid(draft) && !graphicsProblem;
  const submit = () => {
    if (!valid) return;
    dispatch({
      type: "SET_PREFERENCES",
      prefs: sanitizePreferences({
        ...draft,
        rotationStepDeg: sanitizeRotationStep(Number(rotationText), state.prefs.rotationStepDeg),
        // `m_rotationAngle.GetAngleValue()`: the Footprint Editor's own step
        fpRotationStepDeg: sanitizeRotationStep(Number(fpRotationText), state.prefs.fpRotationStepDeg),
      }),
    });
    for (const e of EDITOR_WINDOWS) setWindowOptions(e, windows[e]);
    // the line mode is the Footprint Editor's (`m_AngleSnapMode`)
    if (fpExtra.lineMode !== fpState.lineMode) fpDispatch({ type: "SET_LINE_MODE", mode: fpExtra.lineMode });
    colors.commit();
    setViewer3DPrefs(sanitizeViewer3DPrefs(draft3d));
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

  /** `PANEL_GAL_OPTIONS`: an editor's Grid Display and Cursor. */
  const displayPage = (editor: EditorWindow) => {
    const w = windows[editor];
    const setW = (patch: Partial<WindowOptions>) => setWindows((all) => ({ ...all, [editor]: { ...all[editor], ...patch } }));
    const radio = (name: string, label: string, checked: boolean, onChange: () => void, testid?: string) => (
      <label style={{ marginRight: 12 }}>
        <input type="radio" name={`${name}-${editor}`} data-testid={testid} checked={checked} onChange={onChange} /> {label}
      </label>
    );
    return (
      <div data-testid={`display-options-${editor}`}>
        {heading("Grid Display")}
        <div className="filter-row">
          <span style={{ width: 170 }}>Style:</span>
          {radio("grid-style", "Dots", w.gridStyle === "dots", () => setW({ gridStyle: "dots" }), "pref-grid-dots")}
          {radio("grid-style", "Lines", w.gridStyle === "lines", () => setW({ gridStyle: "lines" }), "pref-grid-lines")}
          {radio("grid-style", "Small crosses", w.gridStyle === "small-cross", () => setW({ gridStyle: "small-cross" }), "pref-grid-crosses")}
        </div>
        <div className="filter-row">
          <span style={{ width: 170 }}>Grid thickness:</span>
          <select aria-label="Grid thickness" data-testid="pref-grid-thickness" value={String(w.gridLineWidth)} onChange={(e) => setW({ gridLineWidth: Number(e.target.value) })}>
            {GRID_THICKNESS_CHOICES.map((t) => (
              <option key={t} value={String(t)}>
                {t.toFixed(1)}
              </option>
            ))}
          </select>
          <span>pixels</span>
        </div>
        <div className="filter-row">
          <span style={{ width: 170 }}>Minimum grid spacing:</span>
          <input
            type="number"
            aria-label="Minimum grid spacing"
            data-testid="pref-grid-min-spacing"
            style={{ width: 70 }}
            min={GRID_MIN_SPACING_RANGE.min}
            max={GRID_MIN_SPACING_RANGE.max}
            step={GRID_MIN_SPACING_RANGE.step}
            value={w.gridMinSpacing}
            onChange={(e) => setW({ gridMinSpacing: gridMinSpacingValue(Number(e.target.value)) })}
          />
          <span>pixels</span>
        </div>
        {heading("Cursor")}
        <div className="filter-row">
          {radio("crosshair", "Small crosshairs", w.crossHairMode === "small", () => setW({ crossHairMode: "small" }))}
          {radio("crosshair", "Full window crosshairs", w.crossHairMode === "full", () => setW({ crossHairMode: "full" }))}
          {radio("crosshair", "45 degree crosshairs", w.crossHairMode === "diag45", () => setW({ crossHairMode: "diag45" }))}
        </div>
        <label className="filter-row">
          <input type="checkbox" checked={w.alwaysShowCursor} onChange={(e) => setW({ alwaysShowCursor: e.target.checked })} />
          Always show crosshairs
        </label>
      </div>
    );
  };

  return (
    <div className="dialog-backdrop" onClick={cancel}>
      <div className="dialog" style={{ width: page === "pcb_colors" ? 600 : 700, maxWidth: "95vw" }} onClick={(e) => e.stopPropagation()} data-testid="preferences-dialog">
        <div className="dialog-header">
          <span>Preferences - {pageTitle(page)}</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", gap: 12 }}>
          {/* The page tree (`PAGED_DIALOG`'s `wxTreebook`): "PCB Editor" is a blank parent page (`book->AddPage( new wxPanel( book ), _( "PCB Editor" ) )`). */}
          <div role="tree" aria-label="Preferences pages" style={{ width: 150, flexShrink: 0, fontSize: 12, borderRight: "1px solid var(--chrome-border)", paddingRight: 8 }}>
            {(
              [
                ["mouse", "Mouse and Touchpad", 0],
                ...EDITOR_WINDOWS.flatMap((e) => [
                  [null, EDITOR_PAGE_LABEL[e], 0],
                  [`display_${e}`, "Display Options", 1],
                  ...(e === "pcb" ? [["pcb_colors", "Colors", 1]] : []),
                  ...(e === "footprint" ? FP_PREFS_SUBTREE : []),
                ]),
                // `book->AddPage( new wxPanel( book ), _( "3D Viewer" ) )` and its pages.
                [null, "3D Viewer", 0],
                ["3d_general", "General", 1],
                ["3d_opengl", "Realtime Rendering", 1],
              ] as [PrefsPage | null, string, number][]
            ).map(([id, label, depth], i) => (
              <div
                key={`${i}-${label}`}
                data-testid={id ? `prefs-page-${id}` : undefined}
                role="treeitem"
                aria-selected={id !== null && page === id}
                data-page={id ?? undefined}
                style={{ padding: "3px 4px", paddingLeft: 4 + depth * 14, cursor: id ? "pointer" : "default", background: id !== null && page === id ? "var(--chrome-accent-bg, rgba(74,163,255,0.2))" : undefined, opacity: id ? 1 : 0.75 }}
                onClick={() => id && setPage(id)}
              >
                {label}
              </div>
            ))}
          </div>
          <div style={{ flex: 1, minWidth: 0 }}>
            {page === "pcb_colors" && colors.element}
            {page.startsWith("display_") && displayPage(page.slice("display_".length) as EditorWindow)}
            {page === "3d_general" && <Viewer3DGeneralPage draft={draft3d} set={(patch) => setDraft3d((d) => ({ ...d, ...patch }))} />}
            {page === "3d_opengl" && <Viewer3DOpenGLPage draft={draft3d} set={(patch) => setDraft3d((d) => ({ ...d, ...patch }))} />}
            {isFpPage(page) && (
              <FpPreferencesPage page={page} draft={draft} set={set} extra={fpExtra} setExtra={setFpExtra} rotationText={fpRotationText} setRotationText={setFpRotationText} units={state.units} />
            )}
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
              <div className="filter-row" title="How fast to pan when moving an object off the edge of the screen">
                <span style={{ width: 120 }}>Auto pan speed:</span>
                <input type="range" min={1} max={10} step={1} aria-label="Auto pan speed" value={draft.autoPanAcceleration} onChange={(e) => set({ autoPanAcceleration: Number(e.target.value) })} />
                <span style={{ width: 20, textAlign: "right" }}>{draft.autoPanAcceleration}</span>
              </div>

              {/* `PANEL_MOUSE_SETTINGS`' "Drag Gestures": every canvas reads them (kicad-port/viewControls.ts `leftDragOutcome`, `dragActionOf`, `motionPan`) */}
              {heading("Drag Gestures")}
              <div className="filter-row">
                <span style={{ width: 210 }}>Left button drag:</span>
                <select aria-label="Left button drag" data-testid="pref-drag-left" value={draft.dragLeft} onChange={(e) => set({ dragLeft: e.target.value as Preferences["dragLeft"] })}>
                  <option value="select">Draw selection rectangle</option>
                  <option value="drag_selected">Drag selected objects; otherwise draw selection rectangle</option>
                  <option value="drag_any">Drag any object (selected or not)</option>
                </select>
              </div>
              <div className="filter-row">
                <span style={{ width: 210 }}>Middle button drag:</span>
                <select aria-label="Middle button drag" data-testid="pref-drag-middle" value={draft.dragMiddle} onChange={(e) => set({ dragMiddle: e.target.value as Preferences["dragMiddle"] })}>
                  <option value="pan">Pan</option>
                  <option value="zoom">Zoom</option>
                  <option value="none">None</option>
                </select>
              </div>
              <div className="filter-row">
                <span style={{ width: 210 }}>Right button drag:</span>
                <select aria-label="Right button drag" data-testid="pref-drag-right" value={draft.dragRight} onChange={(e) => set({ dragRight: e.target.value as Preferences["dragRight"] })}>
                  <option value="pan">Pan</option>
                  <option value="zoom">Zoom</option>
                  <option value="none">None</option>
                </select>
              </div>
              <div className="filter-row">
                <span style={{ width: 210 }}>Pan on mouse movement with key:</span>
                <select aria-label="Pan on mouse movement with key" data-testid="pref-motion-pan" value={draft.motionPanModifier} onChange={(e) => set({ motionPanModifier: e.target.value as Preferences["motionPanModifier"] })}>
                  <option value="none">None</option>
                  <option value="alt">{isMac() ? "Option" : "Alt"}</option>
                  <option value="ctrl">{isMac() ? "Cmd" : "Ctrl"}</option>
                  <option value="shift">Shift</option>
                </select>
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

              {heading("Editing Options (Symbol Editor): Repeated Items")}
              <div className="filter-row" title="How far Repeat Pin puts the next pin (forced to a multiple of 25 mils).">
                <span style={{ width: 170 }}>Pitch of repeated pins:</span>
                <input
                  type="number"
                  aria-label="Pitch of repeated pins"
                  data-testid="pref-sym-pin-pitch"
                  style={{ width: 70 }}
                  step={25}
                  value={draft.symRepeatPinStepMils}
                  onChange={(e) => set({ symRepeatPinStepMils: Number(e.target.value) })}
                  onBlur={() => set({ symRepeatPinStepMils: sanitizePinStepMils(draft.symRepeatPinStepMils, state.prefs.symRepeatPinStepMils) })}
                />
                <span>mils</span>
              </div>
              <div className="filter-row" title="What Repeat Pin adds to the pin's number and name.">
                <span style={{ width: 170 }}>Label increment:</span>
                <input type="number" aria-label="Label increment" data-testid="pref-sym-label-increment" style={{ width: 70 }} min={-10} max={10} step={1} value={draft.symRepeatLabelDelta} onChange={(e) => set({ symRepeatLabelDelta: Number(e.target.value) })} />
              </div>

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
              <label className="filter-row" title="Pan the canvas left and right when scrolling left to right on the touchpad">
                <input type="checkbox" data-testid="pref-horizontal-pan" checked={draft.horizontalPan} onChange={(e) => set({ horizontalPan: e.target.checked })} />
                Pan left/right with horizontal movement
              </label>
              <div style={{ minHeight: 18, marginTop: 4, fontSize: 12, color: "#ff6666" }}>{valid ? "" : "Only one action can be assigned to each column"}</div>
              <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
                <button onClick={() => setDraft((d) => ({ ...d, ...MOUSE_SCROLL_DEFAULTS }))}>Reset to Mouse Defaults</button>
                <button onClick={() => setDraft((d) => ({ ...d, ...TRACKPAD_SCROLL_DEFAULTS }))}>Reset to Trackpad Defaults</button>
              </div>
              <div style={{ fontSize: 11, opacity: 0.65, marginTop: 10 }}>
                Not offered in the browser: center and warp cursor on zoom (a page cannot move the pointer). A touchpad pinch zooms about the pointer whatever the scroll settings say, as in KiCad.
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
          {/* `PANEL_FP_EDITOR_GRAPHICS_DEFAULTS::ResetPanel`: KiCad's factory table */}
          {page === "fp_graphics" && (
            <button style={{ marginRight: "auto" }} onClick={() => set({ fpTextGraphics: null })} data-testid="fp-prefs-graphics-reset" title={`Reset to ${FACTORY_TEXT_GRAPHICS.silk.line_width_nm / 1e6} mm silkscreen lines and KiCad's other factory values`}>
              Reset Graphics Defaults to Defaults
            </button>
          )}
          <button onClick={cancel}>Cancel</button>
          <button className="primary" disabled={!valid} onClick={submit} data-testid="preferences-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

const heading3d = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;

/**
 * `PANEL_3D_DISPLAY_OPTIONS` (Preferences > 3D Viewer > General): its Camera Options -- "Rotation increment:" (`camera.rotation_increment`, 0 .. 359 deg), "Redraw while
 * moving" (`camera.animation_enabled`) and "Redraw speed:" (`camera.moving_speed_multiplier`, 1 .. 5). Its Render Options are not offered: the silkscreen is not
 * clipped at via annuli or at the mask's edges here, plated and bare copper are one colour, and the models keep their own materials; "Show filled areas in zones" is
 * the Appearance manager's Zones row.
 */
function Viewer3DGeneralPage({ draft, set }: { draft: Viewer3DPrefs; set: (patch: Partial<Viewer3DPrefs>) => void }) {
  return (
    <div className="dialog-body" style={{ flex: 1 }} data-page-body="3d_general">
      {heading3d("Camera Options")}
      <div className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }} title="Set increment (in degrees) for the Rotate commands.">
        <span>Rotation increment:</span>
        <input type="number" aria-label="Rotation increment" min={0} max={359} step={1} value={draft.rotationIncrementDeg} style={{ width: 70 }} onChange={(e) => set({ rotationIncrementDeg: Number(e.target.value) })} />
        <span>deg</span>
      </div>
      <label className="filter-row">
        <input type="checkbox" aria-label="Redraw while moving" checked={draft.animationEnabled} onChange={(e) => set({ animationEnabled: e.target.checked })} />
        Redraw while moving
      </label>
      <div className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6, opacity: draft.animationEnabled ? 1 : 0.5 }}>
        <span>Redraw speed:</span>
        <input type="range" aria-label="Redraw speed" min={1} max={5} step={1} value={draft.movingSpeedMultiplier} disabled={!draft.animationEnabled} onChange={(e) => set({ movingSpeedMultiplier: Number(e.target.value) })} />
        <span>{draft.movingSpeedMultiplier}</span>
      </div>
      <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
        <button onClick={() => set({ rotationIncrementDeg: DEFAULT_VIEWER3D_PREFS.rotationIncrementDeg, animationEnabled: DEFAULT_VIEWER3D_PREFS.animationEnabled, movingSpeedMultiplier: DEFAULT_VIEWER3D_PREFS.movingSpeedMultiplier })}>Reset to Defaults</button>
      </div>
      <div style={{ fontSize: 11, opacity: 0.65, marginTop: 10 }}>
        Not offered: the Render Options (clip silkscreen at via annuli and at solder mask edges, bare copper colours, material properties). Show filled areas in zones is the
        Appearance manager&apos;s Zones row.
      </div>
    </div>
  );
}

/**
 * `PANEL_3D_OPENGL_OPTIONS` (Preferences > 3D Viewer > Realtime Rendering): "Show copper and tech layers thickness (slow)" (`render.opengl_copper_thickness`),
 * "Highlight items on rollover" (`render.opengl_highlight_on_rollover`), "Anti-aliasing:" (`render.opengl_AA_mode`; the browser's multisampling is on or off, so
 * 2x, 4x and 8x are all on) and "Selection color:" (`render.opengl_selection_color`). "Show model bounding boxes" is the Appearance manager's row of that name; the
 * While Moving options are not offered (the view does not drop detail while it moves).
 */
function Viewer3DOpenGLPage({ draft, set }: { draft: Viewer3DPrefs; set: (patch: Partial<Viewer3DPrefs>) => void }) {
  return (
    <div className="dialog-body" style={{ flex: 1 }} data-page-body="3d_opengl">
      {heading3d("Rendering Options")}
      <label className="filter-row">
        <input type="checkbox" aria-label="Show copper and tech layers thickness" checked={draft.copperThickness} onChange={(e) => set({ copperThickness: e.target.checked })} />
        Show copper and tech layers thickness (slow)
      </label>
      <label className="filter-row">
        <input type="checkbox" aria-label="Highlight items on rollover" checked={draft.highlightOnRollover} onChange={(e) => set({ highlightOnRollover: e.target.checked })} />
        Highlight items on rollover
      </label>
      <div className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span>Anti-aliasing:</span>
        <select aria-label="Anti-aliasing" value={draft.antialiasing} onChange={(e) => set({ antialiasing: e.target.value as AntialiasingMode })}>
          <option value="none">Disabled</option>
          <option value="2x">2x</option>
          <option value="4x">4x</option>
          <option value="8x">8x</option>
        </select>
      </div>
      <div style={{ fontSize: 11, opacity: 0.75, marginLeft: 4 }}>3D-Viewer must be closed and re-opened to apply this setting</div>
      <div className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span>Selection color:</span>
        <input type="color" aria-label="Selection color" value={draft.selectionColor} onChange={(e) => set({ selectionColor: e.target.value })} />
      </div>
      <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
        <button onClick={() => set({ copperThickness: DEFAULT_VIEWER3D_PREFS.copperThickness, highlightOnRollover: DEFAULT_VIEWER3D_PREFS.highlightOnRollover, antialiasing: DEFAULT_VIEWER3D_PREFS.antialiasing, selectionColor: DEFAULT_VIEWER3D_PREFS.selectionColor })}>Reset to Defaults</button>
      </div>
      <div style={{ fontSize: 11, opacity: 0.65, marginTop: 10 }}>Show model bounding boxes is the Appearance manager&apos;s row. Not offered: the While Moving options (the view keeps its detail while it moves).</div>
    </div>
  );
}
