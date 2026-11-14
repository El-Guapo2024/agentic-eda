// KiCad's Preferences dialog (`PAGED_DIALOG`, `EDA_BASE_FRAME::ShowPreferences` in common/eda_base_frame.cpp at KiCad 8303b2ad), its tree in KiCad's order:
//
//   Common (components/CommonPreferencesPages.tsx, `PANEL_COMMON_SETTINGS`), Mouse and Touchpad (below, `PANEL_MOUSE_SETTINGS`), Hotkeys
//   (components/HotkeysPage.tsx, `PANEL_HOTKEYS_EDITOR`);
//   Symbol Editor > Display Options, Grids, Editing Options, Colors, Toolbars;
//   Schematic Editor > Display Options, Grids, Editing Options, Colors, Toolbars, Field Name Templates, Simulator (components/SchPreferencesPages.tsx);
//   Footprint Editor > Display Options, Grids, Origins & Axes, Editing Options, Colors, Toolbars, Footprint Defaults, Graphics Defaults, User Layer Names;
//   PCB Editor > Display Options, Grids, Origins & Axes (components/PcbOriginsAxesPage.tsx), Editing Options, Colors, Toolbars;
//   3D Viewer > General, Toolbars, Realtime Renderer;
//   Maintenance (`PANEL_MAINTENANCE`).
//
// An editor's name is a blank parent page (`book->AddPage( new wxPanel( book ), _( "PCB Editor" ) )`). KiCad's other pages are not here: SpaceMouse, Version
// Control, Packages and Updates, Plugins, the Schematic Editor's Data Sources (ODBC, out of a browser's reach), the 3D
// Viewer's Raytracing Renderer, and the Gerber Viewer's and Drawing Sheet Editor's pages (no such editors).
//
// Each editor's Display Options is the Grid Display and Cursor part of `PANEL_GAL_OPTIONS` (common/dialogs/panel_gal_options.cpp: Dots / Lines / Small crosses,
// "Grid thickness:" 0.5 to 10 pixels, "Minimum grid spacing:" 5 to 200 pixels, "Snap to grid:" Always / When grid shown / Never, the three crosshairs and
// "Always show crosshairs"; each editor keeps its own, state/commonOptions.ts `window`). The Grids pages are Edit Grids' (components/GridsDialog.tsx,
// `PANEL_GRID_SETTINGS`), the Editing Options pages components/EditingOptionsPages.tsx (and the Footprint Editor's in FpPreferencesPages.tsx), the Colors pages
// PcbColorsPage.tsx and EditorColorsPages.tsx, the Toolbars pages ToolbarsPage.tsx, the 3D Viewer's at the end of this file.
//
// The dialog: "Preferences - <page>" (the title of the page shown), reopened on the page it was left on (`g_lastPage`), OK runs every page's
// `TransferDataFromWindow` (each page hook's `commit`, components/preferencesPage.ts), Cancel none; the reset button "Reset <page> to Defaults" is the shown
// page's `ResetPanel` ("Reset to Defaults", greyed, on a page that has none: `PAGED_DIALOG::UpdateResetButton`), with the page's tooltip ("Reset all settings
// on this page to their default", `RESETTABLE_PANEL::GetResetTooltip`); the info bar shows a page's message (`GetInfoBar()->ShowMessageFor`).
//
// Mouse and Touchpad (common/dialogs/panel_mouse_settings.cpp): edits `state.prefs` (kicad-port/preferences.ts), which every canvas's wheel, drag and auto-pan
// handling reads -- see that file's header for exactly which settings are offered. Same behaviours as the C++: the scroll-gesture grid keeps one action per
// column (`OnScrollRadioButton` moves a displaced action to the first free column), a clash that remains (two "--") shows the warning and refuses OK
// (`TransferDataFromWindow`), and "Reset to Mouse/Trackpad Defaults" sets the grid (`onMouseDefaults`, `onTrackpadDefaults`). On macOS the Ctrl column reads
// "Cmd" and Alt reads "Option", as in the C++.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { NET_NAMES_CHOICES, TRACK_CLEARANCE_CHOICES, sanitizeNetNamesMode, sanitizeTrackClearanceMode } from "../kicad-port/pcbDisplayOptions";
import { assignScrollModifier, DEFAULT_PREFERENCES, MOUSE_SCROLL_DEFAULTS, sanitizePreferences, sanitizeRotationStep, scrollModSetValid, TRACKPAD_SCROLL_DEFAULTS, type Preferences, type ScrollModifier, type ScrollRow } from "../kicad-port/preferences";
import { isMac } from "../platform";
import { usePcbColorsPage } from "./PcbColorsPage";
import { PCB_AXES_FIELDS, PcbOriginsAxesPage } from "./PcbOriginsAxesPage";
import { FP_PREFS_TITLES, FpPreferencesPage, fpGraphicsProblem, type FpPrefsExtra, type FpPrefsPage } from "./FpPreferencesPages";
import { useFpDispatch, useFpState } from "../state/footprintEditorStore";
import { FACTORY_TEXT_GRAPHICS } from "../kicad-port/layerDefaults";
import { DEFAULT_WINDOW_OPTIONS, EDITOR_WINDOWS, getCommonOptions, setWindowOptions, type EditorWindow, type WindowOptions } from "../state/commonOptions";
import type { GridSnapping } from "../kicad-port/windowOptions";
import { GRID_MIN_SPACING_RANGE, GRID_THICKNESS_CHOICES, gridMinSpacingValue } from "../kicad-port/grid";
import { getViewer3DUserSettings, setViewer3DPrefs } from "../state/viewer3dUserSettings";
import { DEFAULT_VIEWER3D_PREFS, sanitizeViewer3DPrefs, type AntialiasingMode, type MaterialMode, type Viewer3DPrefs } from "../kicad-port/viewer3dSettings";
import { useCommonPage, useMaintenancePage } from "./CommonPreferencesPages";
import { useHotkeysPage } from "./HotkeysPage";
import { useEditorColorsPage } from "./EditorColorsPages";
import { useToolbarsPage } from "./ToolbarsPage";
import { useFpFieldDefaultsPage, useFpLayerNamesPage } from "./FpDefaultsPages";
import { PcbEditingOptionsPage, SchEditingOptionsPage, SymEditingOptionsPage, type EditingExtra } from "./EditingOptionsPages";
import { GridsPageBody, useGridsPages } from "./GridsDialog";
import { takePreferencesStartPage } from "../state/preferencesStart";
import type { GridEditor } from "../kicad-port/gridSettings";
import type { PreferencesPageHandle } from "./preferencesPage";
import { GlobalFieldNamesPage, SimulatorPrefsPage } from "./SchPreferencesPages";
import { DEFAULT_SIM_WHEEL_ACTIONS } from "../kicad-port/sim/simWheel";
import { transferGlobalTemplates } from "../kicad-port/fieldNameTemplates";
import { WHITESPACE_EXTENDED, WHITESPACE_KEEP, WHITESPACE_REMOVE } from "../kicad-port/schSetup";
import { askConfirm } from "./library/libraryDialogs";
import { useFontFamilies } from "./TextLookControls";
import { fontChoices } from "../kicad-port/textLooks";

/** The pages of the dialog's tree (`PAGED_DIALOG`'s `wxTreebook`). */
type PrefsPage =
  | "common"
  | "mouse"
  | "hotkeys"
  | `display_${EditorWindow}`
  | "sym_grids"
  | "sym_edit"
  | "sym_colors"
  | "sym_toolbars"
  | "sch_grids"
  | "sch_edit"
  | "sch_colors"
  | "sch_toolbars"
  | "sch_field_names"
  | "sch_simulator"
  | "fp_grids"
  | FpPrefsPage
  | "fp_colors"
  | "fp_toolbars"
  | "fp_defaults"
  | "fp_layer_names"
  | "pcb_grids"
  | "pcb_axes"
  | "pcb_edit"
  | "pcb_colors"
  | "pcb_toolbars"
  | "3d_general"
  | "3d_toolbars"
  | "3d_opengl"
  | "maintenance";

/** The tree as `ShowPreferences` builds it: [page or null for an editor's blank parent page, label, depth]. */
const TREE: ReadonlyArray<[PrefsPage | null, string, number]> = [
  ["common", "Common", 0],
  ["mouse", "Mouse and Touchpad", 0],
  ["hotkeys", "Hotkeys", 0],
  [null, "Symbol Editor", 0],
  ["display_symbol", "Display Options", 1],
  ["sym_grids", "Grids", 1],
  ["sym_edit", "Editing Options", 1],
  ["sym_colors", "Colors", 1],
  ["sym_toolbars", "Toolbars", 1],
  [null, "Schematic Editor", 0],
  ["display_schematic", "Display Options", 1],
  ["sch_grids", "Grids", 1],
  ["sch_edit", "Editing Options", 1],
  ["sch_colors", "Colors", 1],
  ["sch_toolbars", "Toolbars", 1],
  ["sch_field_names", "Field Name Templates", 1],
  ["sch_simulator", "Simulator", 1],
  [null, "Footprint Editor", 0],
  ["display_footprint", "Display Options", 1],
  ["fp_grids", "Grids", 1],
  ["fp_axes", FP_PREFS_TITLES.fp_axes, 1],
  ["fp_edit", FP_PREFS_TITLES.fp_edit, 1],
  ["fp_colors", "Colors", 1],
  ["fp_toolbars", "Toolbars", 1],
  ["fp_defaults", "Footprint Defaults", 1],
  ["fp_graphics", FP_PREFS_TITLES.fp_graphics, 1],
  ["fp_layer_names", "User Layer Names", 1],
  [null, "PCB Editor", 0],
  ["display_pcb", "Display Options", 1],
  ["pcb_grids", "Grids", 1],
  ["pcb_axes", "Origins & Axes", 1],
  ["pcb_edit", "Editing Options", 1],
  ["pcb_colors", "Colors", 1],
  ["pcb_toolbars", "Toolbars", 1],
  [null, "3D Viewer", 0],
  ["3d_general", "General", 1],
  ["3d_toolbars", "Toolbars", 1],
  ["3d_opengl", "Realtime Renderer", 1],
  ["maintenance", "Maintenance", 0],
];

/** Each page's title in the dialog's caption (`GetPageText`). */
function pageTitle(page: PrefsPage): string {
  return TREE.find(([id]) => id === page)?.[1] ?? page;
}

/** The Grids pages and the editor each is of. */
const GRIDS_PAGE: Partial<Record<PrefsPage, GridEditor>> = { sym_grids: "symbol", sch_grids: "schematic", fp_grids: "footprint", pcb_grids: "pcb" };

const isFpPage = (p: string): p is FpPrefsPage => p === "fp_axes" || p === "fp_edit" || p === "fp_graphics";

/** `PAGED_DIALOG` opens on the page it was last closed on (its `g_lastPage` by title). */
let lastPage: PrefsPage = "common";

const COLUMNS: { value: ScrollModifier; label: string }[] = [
  { value: "none", label: "--" },
  { value: "ctrl", label: isMac() ? "Cmd" : "Ctrl" },
  { value: "shift", label: "Shift" },
  { value: "alt", label: isMac() ? "Option" : "Alt" },
];

const ROW_KEY: Record<ScrollRow, "scrollModifierZoom" | "scrollModifierPanH" | "scrollModifierPanV"> = { zoom: "scrollModifierZoom", panH: "scrollModifierPanH", panV: "scrollModifierPanV" };

/** The fields of `Preferences` each page owns, for its `ResetPanel`. */
const MOUSE_FIELDS: ReadonlyArray<keyof Preferences> = ["scrollModifierZoom", "scrollModifierPanH", "scrollModifierPanV", "reverseScrollZoom", "reverseScrollPanH", "autoPan", "autoPanAcceleration", "dragMiddle", "dragRight", "dragLeft", "motionPanModifier", "horizontalPan", "zoomAcceleration", "zoomSpeedAuto", "zoomSpeed"];
const PCB_EDIT_FIELDS: ReadonlyArray<keyof Preferences> = ["rotationStepDeg", "pcbArcEditMode", "flipDirection", "allowFreePads", "magneticPads", "magneticTracks", "magneticGraphics", "ratsnestThicknessPx", "escClearsNetHighlight", "showCourtyardCollisions"];
const SCH_EDIT_FIELDS: ReadonlyArray<keyof Preferences> = [
  "schArcEditMode",
  "schDragIsMove",
  "schAutoStartWires",
  "schEscClearsNetHighlight",
  "allowUnconstrainedPinSwaps",
  "schSheetBorderColor",
  "schSheetBackgroundColor",
  "schNewPowerSymbols",
  "schAutoplaceFields",
  "schAutoplaceJustify",
  "schAutoplaceAlign",
  "schRepeatOffsetXMils",
  "schRepeatOffsetYMils",
  "schRepeatLabelIncrement",
  "schFootprintPreview",
  "schRescueNeverShow",
];
const SYM_EDIT_FIELDS: ReadonlyArray<keyof Preferences> = ["symDefaultLineWidthMils", "symDefaultTextSizeMils", "symDefaultPinLengthMils", "symDefaultPinNumSizeMils", "symDefaultPinNameSizeMils", "symRepeatPinStepMils", "symRepeatLabelDelta", "symDragPinsWithEdges"];

function defaultsOf(fields: ReadonlyArray<keyof Preferences>): Partial<Preferences> {
  const out: Partial<Preferences> = {};
  for (const f of fields) (out as Record<string, unknown>)[f] = DEFAULT_PREFERENCES[f];
  return out;
}

const RESET_TOOLTIP = "Reset all settings on this page to their default";

export function PreferencesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.preferencesDialogOpen;
  const [draft, setDraft] = useState<Preferences>(state.prefs);
  // The step is typed as text and taken when the dialog is accepted: `m_rotationAngle.GetAngleValue()` in `TransferDataFromWindow`, which keeps the old step for a value that is nothing.
  const [rotationText, setRotationText] = useState(String(state.prefs.rotationStepDeg));
  const [page, setPageState] = useState<PrefsPage>(lastPage);
  /** The pages shown since the dialog opened: KiCad builds most pages lazily (`AddLazySubPage`), and a page never shown is never transferred on OK. */
  const [shownPages, setShownPages] = useState<ReadonlySet<PrefsPage>>(new Set([lastPage]));
  const setPage = (p: PrefsPage) => {
    lastPage = p;
    setPageState(p);
    setShownPages((s) => (s.has(p) ? s : new Set(s).add(p)));
    const g = GRIDS_PAGE[p];
    if (g) grids.show(g);
  };
  // The info bar (`PAGED_DIALOG::GetInfoBar`): a page's message for six seconds.
  const [info, setInfo] = useState<string | null>(null);
  const infoTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const showInfo = (message: string) => {
    setInfo(message);
    if (infoTimer.current) clearTimeout(infoTimer.current);
    infoTimer.current = setTimeout(() => setInfo(null), 6000);
  };
  useEffect(() => () => void (infoTimer.current && clearTimeout(infoTimer.current)), []);

  const colors = usePcbColorsPage(open, page === "pcb_colors");
  const common = useCommonPage(open, page === "common");
  const maintenance = useMaintenancePage(open, page === "maintenance", showInfo);
  const hotkeys = useHotkeysPage(open, page === "hotkeys");
  const schColors = useEditorColorsPage("schematic", open, page === "sch_colors");
  const symColors = useEditorColorsPage("symbol", open, page === "sym_colors");
  const fpColors = useEditorColorsPage("footprint", open, page === "fp_colors");
  const toolbars = {
    pcb_toolbars: useToolbarsPage("pcb", open, page === "pcb_toolbars"),
    sch_toolbars: useToolbarsPage("schematic", open, page === "sch_toolbars"),
    fp_toolbars: useToolbarsPage("footprint", open, page === "fp_toolbars"),
    sym_toolbars: useToolbarsPage("symbol", open, page === "sym_toolbars"),
    "3d_toolbars": useToolbarsPage("3d", open, page === "3d_toolbars"),
  };
  const fpDefaults = useFpFieldDefaultsPage(open, page === "fp_defaults");
  const fpLayerNames = useFpLayerNamesPage(open, page === "fp_layer_names");
  const grids = useGridsPages(GRIDS_PAGE[lastPage] ?? null);
  /** The pages whose state lives in a hook of their own, by id. */
  const handles: Partial<Record<PrefsPage, PreferencesPageHandle>> = {
    common,
    maintenance,
    hotkeys,
    sch_colors: schColors,
    sym_colors: symColors,
    fp_colors: fpColors,
    ...toolbars,
    fp_defaults: fpDefaults,
    fp_layer_names: fpLayerNames,
  };
  const allHandles = [colors as PreferencesPageHandle, ...Object.values(handles)] as PreferencesPageHandle[];

  // The Footprint Editor's pages (components/FpPreferencesPages.tsx): its rotation step as typed, and its line mode, which they edit beside the preferences.
  const fpState = useFpState();
  const fpDispatch = useFpDispatch();
  const [fpRotationText, setFpRotationText] = useState(String(state.prefs.fpRotationStepDeg));
  const [fpExtra, setFpExtraState] = useState<FpPrefsExtra>({ lineMode: fpState.lineMode });
  const setFpExtra = (patch: Partial<FpPrefsExtra>) => setFpExtraState((e) => ({ ...e, ...patch }));
  // What the board's and the schematic's Editing Options edit outside the preferences (their line modes, the ratsnest's curves).
  const editingExtraNow = (): EditingExtra => ({ pcbConstrain45: state.pcbx.angleSnapMode !== "direct", ratsnestCurved: state.ratsnestCurved, schLineMode: state.schLineMode });
  const [editingExtra, setEditingExtraState] = useState<EditingExtra>(editingExtraNow);
  // Display Options' "Show pad numbers" (`m_ViewersDisplay.m_DisplayPadNumbers`), which the studio keeps beside the preferences (`bcx.showPadNumbers`, the same switch as
  // `pcbnew.Control.showPadNumbers`)
  const [padNumbers, setPadNumbers] = useState(state.bcx.showPadNumbers);
  const setEditingExtra = (patch: Partial<EditingExtra>) => setEditingExtraState((e) => ({ ...e, ...patch }));
  // each editor's Display Options as edited so far (`PANEL_GAL_OPTIONS` of its frame); OK keeps them
  const [windows, setWindows] = useState<Record<EditorWindow, WindowOptions>>(getCommonOptions().window);
  /** The 3D viewer's settings being edited (state/viewer3dUserSettings.ts), taken on OK as the other pages are. */
  const [draft3d, setDraft3d] = useState<Viewer3DPrefs>(getViewer3DUserSettings().prefs);
  // the machine's font families, for Display Options' "Default font:"
  const fontFamilies = useFontFamilies();

  useEffect(() => {
    if (open) {
      // `ShowPreferences( aStartPage, aStartParentPage )`: a page a tool asked for (Edit Grids: the editor's Grids page), else the last one (`g_lastPage`)
      const start = takePreferencesStartPage();
      if (start) lastPage = (Object.keys(GRIDS_PAGE) as PrefsPage[]).find((p) => GRIDS_PAGE[p] === start.grids) ?? lastPage;
      setDraft(state.prefs);
      setRotationText(String(state.prefs.rotationStepDeg));
      setFpRotationText(String(state.prefs.fpRotationStepDeg));
      setFpExtraState({ lineMode: fpState.lineMode });
      setEditingExtraState(editingExtraNow());
      setPadNumbers(state.bcx.showPadNumbers);
      setPageState(lastPage);
      setShownPages(new Set([lastPage]));
      setWindows(getCommonOptions().window);
      setDraft3d(getViewer3DUserSettings().prefs);
      grids.restart(GRIDS_PAGE[lastPage] ?? null);
      setInfo(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_PREFERENCES_DIALOG_OPEN", open: false });
  const cancel = () => {
    for (const h of allHandles) h.cancel();
    close();
  };
  const graphicsProblem = fpGraphicsProblem(draft.fpTextGraphics);
  const scrollValid = scrollModSetValid(draft);
  const valid = scrollValid && !graphicsProblem && allHandles.every((h) => h.valid !== false);
  const submit = async () => {
    if (!valid) return;
    // Field Name Templates' `TransferDataFromWindow` (a lazy page: only when it was shown): each name with white space at an end is asked about, then the rows
    // with a name are kept as `AddTemplateFieldName` keeps them (kicad-port/fieldNameTemplates.ts)
    const fieldNames = shownPages.has("sch_field_names")
      ? await transferGlobalTemplates(draft.schGlobalFieldNames, (message) => askConfirm({ title: "Warning", message: `${message}\n\n${WHITESPACE_EXTENDED}`, okLabel: WHITESPACE_REMOVE, cancelLabel: WHITESPACE_KEEP }))
      : draft.schGlobalFieldNames;
    dispatch({
      type: "SET_PREFERENCES",
      prefs: sanitizePreferences({
        ...draft,
        schGlobalFieldNames: fieldNames,
        rotationStepDeg: sanitizeRotationStep(Number(rotationText), state.prefs.rotationStepDeg),
        // `m_rotationAngle.GetAngleValue()`: the Footprint Editor's own step
        fpRotationStepDeg: sanitizeRotationStep(Number(fpRotationText), state.prefs.fpRotationStepDeg),
      }),
    });
    for (const e of EDITOR_WINDOWS) setWindowOptions(e, windows[e]);
    // the line mode is the Footprint Editor's (`m_AngleSnapMode`)
    if (fpExtra.lineMode !== fpState.lineMode) fpDispatch({ type: "SET_LINE_MODE", mode: fpExtra.lineMode });
    // the board's `m_AngleSnapMode = m_cbConstrainHV45Mode->GetValue() ? LEADER_MODE::DEG45 : LEADER_MODE::DIRECT` (written whatever it was), and its ratsnest's curves;
    // the schematic's `m_Drawing.line_mode = m_choiceLineMode->GetSelection()`
    // (only when the page was shown: a lazy page never built is never transferred)
    const angle = editingExtra.pcbConstrain45 ? "45" : "direct";
    if (shownPages.has("pcb_edit") && angle !== state.pcbx.angleSnapMode) dispatch({ type: "PCBX", patch: { angleSnapMode: angle } });
    if (editingExtra.ratsnestCurved !== state.ratsnestCurved) dispatch({ type: "TOGGLE_RATSNEST_CURVED" });
    if (editingExtra.schLineMode !== state.schLineMode) dispatch({ type: "SET_SCH_LINE_MODE", mode: editingExtra.schLineMode });
    if (shownPages.has("display_pcb") && padNumbers !== state.bcx.showPadNumbers) dispatch({ type: "BCX", patch: { showPadNumbers: padNumbers } });
    colors.commit();
    for (const h of Object.values(handles)) h?.commit();
    grids.commit();
    setViewer3DPrefs(sanitizeViewer3DPrefs(draft3d));
    close();
  };
  const set = (patch: Partial<Preferences>) => setDraft((d) => ({ ...d, ...patch }));

  /** The shown page's `ResetPanel`, or null for a page without one (`wxRESETTABLE`). */
  const pageReset = (): { run: () => void; tooltip: string } | null => {
    const h = page === "pcb_colors" ? (colors as PreferencesPageHandle) : handles[page];
    if (h) return h.reset ? { run: h.reset, tooltip: h.resetTooltip ?? (page === "pcb_colors" ? "Reset all colors in this theme to the KiCad defaults" : RESET_TOOLTIP) } : null;
    const g = GRIDS_PAGE[page];
    if (g) return { run: () => grids.reset(g), tooltip: RESET_TOOLTIP };
    if (page === "mouse") return { run: () => set(defaultsOf(MOUSE_FIELDS)), tooltip: RESET_TOOLTIP };
    if (page === "pcb_edit")
      return {
        run: () => {
          set(defaultsOf(PCB_EDIT_FIELDS));
          setRotationText(String(DEFAULT_PREFERENCES.rotationStepDeg));
          setEditingExtra({ pcbConstrain45: false, ratsnestCurved: false });
        },
        tooltip: RESET_TOOLTIP,
      };
    if (page === "sch_edit")
      return {
        run: () => {
          set(defaultsOf(SCH_EDIT_FIELDS));
          setEditingExtra({ schLineMode: 1 });
        },
        tooltip: RESET_TOOLTIP,
      };
    if (page === "sym_edit") return { run: () => set(defaultsOf(SYM_EDIT_FIELDS)), tooltip: RESET_TOOLTIP };
    // `PANEL_SIMULATOR_PREFERENCES::ResetPanel`: the mouse defaults
    if (page === "sch_simulator") return { run: () => set({ simWheel: DEFAULT_SIM_WHEEL_ACTIONS }), tooltip: RESET_TOOLTIP };
    // `PANEL_PCBNEW_DISPLAY_ORIGIN::ResetPanel`: the settings of a fresh `PCBNEW_SETTINGS` / `FOOTPRINT_EDITOR_SETTINGS` (the page, X right, Y down)
    if (page === "pcb_axes") return { run: () => set(defaultsOf(PCB_AXES_FIELDS)), tooltip: RESET_TOOLTIP };
    if (page === "fp_axes") return { run: () => set(defaultsOf(["fpInvertXAxis", "fpInvertYAxis"])), tooltip: RESET_TOOLTIP };
    if (page.startsWith("display_")) {
      const e = page.slice("display_".length) as EditorWindow;
      // `PANEL_GAL_OPTIONS::ResetPanel( defaults )`: the Grid Display and the Cursor of a fresh settings file; the board editor's `PANEL_DISPLAY_OPTIONS::ResetPanel`
      // also its clearance outlines, net names and pad numbers (`loadPCBSettings( &cfg )` of a fresh `PCBNEW_SETTINGS`)
      if (e === "pcb") {
        const run = () => {
          set({ pcbNetNames: DEFAULT_PREFERENCES.pcbNetNames, pcbTrackClearance: DEFAULT_PREFERENCES.pcbTrackClearance, pcbPadClearance: DEFAULT_PREFERENCES.pcbPadClearance });
          setPadNumbers(true);
          setWindows((all) => ({ ...all, pcb: { ...all.pcb, gridStyle: DEFAULT_WINDOW_OPTIONS.gridStyle, gridLineWidth: DEFAULT_WINDOW_OPTIONS.gridLineWidth, gridMinSpacing: DEFAULT_WINDOW_OPTIONS.gridMinSpacing, gridSnap: DEFAULT_WINDOW_OPTIONS.gridSnap, crossHairMode: DEFAULT_WINDOW_OPTIONS.crossHairMode, alwaysShowCursor: DEFAULT_WINDOW_OPTIONS.alwaysShowCursor } }));
        };
        return { run, tooltip: RESET_TOOLTIP };
      }
      // the schematic's `PANEL_EESCHEMA_DISPLAY_OPTIONS::ResetPanel` also its Appearance ("Default font:" of a fresh `EESCHEMA_SETTINGS`)
      return { run: () => { if (e === "schematic") set({ schDefaultFont: DEFAULT_PREFERENCES.schDefaultFont }); setWindows((all) => ({ ...all, [e]: { ...all[e], gridStyle: DEFAULT_WINDOW_OPTIONS.gridStyle, gridLineWidth: DEFAULT_WINDOW_OPTIONS.gridLineWidth, gridMinSpacing: DEFAULT_WINDOW_OPTIONS.gridMinSpacing, gridSnap: DEFAULT_WINDOW_OPTIONS.gridSnap, crossHairMode: DEFAULT_WINDOW_OPTIONS.crossHairMode, alwaysShowCursor: DEFAULT_WINDOW_OPTIONS.alwaysShowCursor } })); }, tooltip: RESET_TOOLTIP };
    }
    if (page === "fp_graphics") return { run: () => set({ fpTextGraphics: null }), tooltip: `Reset to ${FACTORY_TEXT_GRAPHICS.silk.line_width_nm / 1e6} mm silkscreen lines and KiCad's other factory values` };
    return null;
  };
  const reset = pageReset();

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
        {/* `m_gridSnapOptions` (`grid.snap`, `GRID_SNAPPING`): every snap of the editor asks `GAL::GetGridSnapping` */}
        <div className="filter-row">
          <span style={{ width: 170 }}>Snap to grid:</span>
          <select aria-label="Snap to grid:" data-testid="pref-grid-snap" value={w.gridSnap} onChange={(e) => setW({ gridSnap: e.target.value as GridSnapping })}>
            <option value="always">Always</option>
            <option value="with_grid">When grid shown</option>
            <option value="never">Never</option>
          </select>
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
        {editor === "pcb" && pcbDisplayOptions()}
        {editor === "schematic" && schDisplayOptions()}
      </div>
    );
  };

  /**
   * `PANEL_EESCHEMA_DISPLAY_OPTIONS`' Appearance "Default font:" (`m_defaultFontCtrl`, a `FONT_CHOICE`: "KiCad Font", then the machine's families): `loadEEschemaSettings` selects
   * `m_Appearance.default_font` (the first entry when it is not in the list), `TransferDataFromWindow` keeps the selection, "KiCad Font" for the first (UI-LEFT-3 S3-29).
   */
  const schDisplayOptions = () => {
    const choices = fontChoices(fontFamilies, null, false).choices;
    const shown = choices.includes(draft.schDefaultFont) ? draft.schDefaultFont : choices[0]!;
    return (
      <>
        {heading("Appearance")}
        <div className="filter-row">
          <span style={{ width: 170 }}>Default font:</span>
          <select aria-label="Default font:" data-testid="pref-sch-default-font" value={shown} onChange={(e) => set({ schDefaultFont: e.target.value })}>
            {choices.map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
        </div>
      </>
    );
  };

  /**
   * `PANEL_DISPLAY_OPTIONS`' board editor part (UI-LEFT-3 P3-1, P3-6; kicad-port/pcbDisplayOptions.ts): "Clearance Outlines" -- "Tracks:" and "Show pad clearance" --
   * and "Annotations" -- "Net names:" and "Show pad numbers". Not here: "Use via color for normal through hole padstacks", "Show all fields when parent footprint is
   * selected", the Cross-probing switches and "Refresh 3D view automatically" (the studio has no such settings yet).
   */
  const pcbDisplayOptions = () => (
    <>
      {heading("Clearance Outlines")}
      <div className="filter-row">
        <span style={{ width: 170 }}>Tracks:</span>
        <select aria-label="Tracks:" data-testid="pref-track-clearance" value={draft.pcbTrackClearance} onChange={(e) => set({ pcbTrackClearance: sanitizeTrackClearanceMode(e.target.value) })}>
          {TRACK_CLEARANCE_CHOICES.map((c) => (
            <option key={c.mode} value={c.mode}>
              {c.label}
            </option>
          ))}
        </select>
      </div>
      <label className="filter-row">
        <input type="checkbox" data-testid="pref-pad-clearance" checked={draft.pcbPadClearance} onChange={(e) => set({ pcbPadClearance: e.target.checked })} />
        Show pad clearance
      </label>
      {heading("Annotations")}
      <div className="filter-row">
        <span style={{ width: 170 }}>Net names:</span>
        <select aria-label="Net names:" data-testid="pref-net-names" value={String(draft.pcbNetNames)} onChange={(e) => set({ pcbNetNames: sanitizeNetNamesMode(Number(e.target.value)) })}>
          {NET_NAMES_CHOICES.map((label, i) => (
            <option key={label} value={String(i)}>
              {label}
            </option>
          ))}
        </select>
      </div>
      <label className="filter-row">
        <input type="checkbox" data-testid="pref-pad-numbers" checked={padNumbers} onChange={(e) => setPadNumbers(e.target.checked)} />
        Show pad numbers
      </label>
    </>
  );

  const mousePage = (): ReactNode => (
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
      <div style={{ minHeight: 18, marginTop: 4, fontSize: 12, color: "#ff6666" }}>{scrollValid ? "" : "Only one action can be assigned to each column"}</div>
      <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
        <button onClick={() => setDraft((d) => ({ ...d, ...MOUSE_SCROLL_DEFAULTS }))}>Reset to Mouse Defaults</button>
        <button onClick={() => setDraft((d) => ({ ...d, ...TRACKPAD_SCROLL_DEFAULTS }))}>Reset to Trackpad Defaults</button>
      </div>
      <div style={{ fontSize: 11, opacity: 0.65, marginTop: 10 }}>
        Not offered in the browser: center and warp cursor on zoom (a page cannot move the pointer). A touchpad pinch zooms about the pointer whatever the scroll settings say, as in KiCad.
      </div>
    </>
  );

  const gridsEditor = GRIDS_PAGE[page];
  const title = pageTitle(page);
  const wide = page === "hotkeys" || page === "common" || page.endsWith("_edit") || page.endsWith("_toolbars") || page === "fp_defaults";

  return (
    <div className="dialog-backdrop" onClick={cancel}>
      <div className="dialog" style={{ width: wide ? 980 : 760, maxWidth: "95vw" }} onClick={(e) => e.stopPropagation()} data-testid="preferences-dialog" role="dialog" aria-label={`Preferences - ${title}`}>
        <div className="dialog-header">
          <span>Preferences - {title}</span>
        </div>
        {info && (
          <div role="status" data-testid="preferences-infobar" style={{ padding: "6px 12px", background: "var(--chrome-accent-bg, rgba(74,163,255,0.15))", fontSize: 12 }}>
            {info}
          </div>
        )}
        <div className="dialog-body" style={{ display: "flex", gap: 12, maxHeight: "75vh" }}>
          {/* The page tree (`PAGED_DIALOG`'s `wxTreebook`): an editor's name is a blank parent page (`book->AddPage( new wxPanel( book ), _( "PCB Editor" ) )`). */}
          <div role="tree" aria-label="Preferences pages" style={{ width: 170, flexShrink: 0, fontSize: 12, borderRight: "1px solid var(--chrome-border)", paddingRight: 8, overflowY: "auto" }}>
            {TREE.map(([id, label, depth], i) => (
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
          <div style={{ flex: 1, minWidth: 0, overflowY: "auto" }} data-page-shown={page}>
            {page === "pcb_colors" && colors.element}
            {handles[page]?.element}
            {page.startsWith("display_") && displayPage(page.slice("display_".length) as EditorWindow)}
            {gridsEditor && <GridsPageBody key={gridsEditor} editor={gridsEditor} page={grids.pages[gridsEditor]} setPage={(next) => grids.setPage(gridsEditor, next)} />}
            {page === "pcb_axes" && <PcbOriginsAxesPage draft={draft} set={set} />}
            {page === "pcb_edit" && <PcbEditingOptionsPage draft={draft} set={set} extra={editingExtra} setExtra={setEditingExtra} rotationText={rotationText} setRotationText={setRotationText} />}
            {page === "sch_edit" && <SchEditingOptionsPage draft={draft} set={set} extra={editingExtra} setExtra={setEditingExtra} rotationText={rotationText} setRotationText={setRotationText} />}
            {page === "sym_edit" && <SymEditingOptionsPage draft={draft} set={set} fallbackPinStep={state.prefs.symRepeatPinStepMils} />}
            {page === "sch_field_names" && <GlobalFieldNamesPage rows={draft.schGlobalFieldNames} setRows={(rows) => set({ schGlobalFieldNames: rows })} />}
            {page === "sch_simulator" && <SimulatorPrefsPage actions={draft.simWheel} setActions={(a) => set({ simWheel: a })} />}
            {page === "3d_general" && <Viewer3DGeneralPage draft={draft3d} set={(patch) => setDraft3d((d) => ({ ...d, ...patch }))} />}
            {page === "3d_opengl" && <Viewer3DOpenGLPage draft={draft3d} set={(patch) => setDraft3d((d) => ({ ...d, ...patch }))} />}
            {isFpPage(page) && (
              <FpPreferencesPage page={page} draft={draft} set={set} extra={fpExtra} setExtra={setFpExtra} rotationText={fpRotationText} setRotationText={setFpRotationText} units={state.units} />
            )}
            {page === "mouse" && mousePage()}
          </div>
        </div>
        <div className="dialog-footer">
          {/* `PAGED_DIALOG`'s reset button: "Reset %s to Defaults" on a page that can be reset, else "Reset to Defaults" greyed (`UpdateResetButton`) */}
          <button style={{ marginRight: "auto" }} disabled={!reset} title={reset?.tooltip} data-testid="preferences-reset" onClick={() => reset?.run()}>
            {reset ? `Reset ${title} to Defaults` : "Reset to Defaults"}
          </button>
          <button onClick={cancel}>Cancel</button>
          <button className="primary" disabled={!valid} onClick={() => void submit()} data-testid="preferences-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

const heading3d = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;

/**
 * `PANEL_3D_DISPLAY_OPTIONS` (Preferences > 3D Viewer > General, KiCad 25ec2335 `panel_3D_display_options_base.cpp`): Render Options -- "Clip silkscreen at via annuli"
 * (`render.clip_silk_on_via_annulus`), "Clip silkscreen at solder mask edges" (`render.subtract_mask_from_silk`), "Use bare copper color for unplated copper (slow)"
 * (`render.differentiate_plated_copper`), "Material properties:" (`render.material_mode`: Realistic, Solid colors, CAD colors) -- and Camera Options: "Rotation increment:"
 * (`camera.rotation_increment`, 0 .. 359 deg), "Redraw while moving" (`camera.animation_enabled`) and "Redraw speed:" (`camera.moving_speed_multiplier`, 1 .. 5). "Show
 * filled areas in zones" (`render.show_zones`) is the Appearance manager's Zones row, which the studio keeps with the board's other rows.
 */
function Viewer3DGeneralPage({ draft, set }: { draft: Viewer3DPrefs; set: (patch: Partial<Viewer3DPrefs>) => void }) {
  return (
    <div className="dialog-body" style={{ flex: 1 }} data-page-body="3d_general">
      {heading3d("Render Options")}
      <label className="filter-row">
        <input type="checkbox" aria-label="Clip silkscreen at via annuli" checked={draft.clipSilkOnViaAnnuli} onChange={(e) => set({ clipSilkOnViaAnnuli: e.target.checked })} />
        Clip silkscreen at via annuli
      </label>
      <label className="filter-row">
        <input type="checkbox" aria-label="Clip silkscreen at solder mask edges" checked={draft.subtractMaskFromSilk} onChange={(e) => set({ subtractMaskFromSilk: e.target.checked })} />
        Clip silkscreen at solder mask edges
      </label>
      <label className="filter-row" title="Use different colors for plated and unplated copper. (Slow)">
        <input type="checkbox" aria-label="Use bare copper color for unplated copper" checked={draft.differentiatePlatedCopper} onChange={(e) => set({ differentiatePlatedCopper: e.target.checked })} />
        Use bare copper color for unplated copper (slow)
      </label>
      <div className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span>Material properties:</span>
        <select aria-label="Material properties" value={draft.materialMode} onChange={(e) => set({ materialMode: e.target.value as MaterialMode })}>
          <option value="realistic">Realistic</option>
          <option value="solid">Solid colors</option>
          <option value="cad">CAD colors</option>
        </select>
      </div>
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
        <button
          onClick={() => {
            const d = DEFAULT_VIEWER3D_PREFS;
            set({ clipSilkOnViaAnnuli: d.clipSilkOnViaAnnuli, subtractMaskFromSilk: d.subtractMaskFromSilk, differentiatePlatedCopper: d.differentiatePlatedCopper, materialMode: d.materialMode, rotationIncrementDeg: d.rotationIncrementDeg, animationEnabled: d.animationEnabled, movingSpeedMultiplier: d.movingSpeedMultiplier });
          }}
        >
          Reset to Defaults
        </button>
      </div>
      <div style={{ fontSize: 11, opacity: 0.65, marginTop: 10 }}>Show filled areas in zones is the Appearance manager&apos;s Zones row.</div>
    </div>
  );
}

/**
 * `PANEL_3D_OPENGL_OPTIONS` (Preferences > 3D Viewer > Realtime Rendering): "Show copper and tech layers thickness (slow)" (`render.opengl_copper_thickness`),
 * "Highlight items on rollover" (`render.opengl_highlight_on_rollover`), "Anti-aliasing:" (`render.opengl_AA_mode`; the browser's multisampling is on or off, so
 * 2x, 4x and 8x are all on) and "Selection color:" (`render.opengl_selection_color`); While Moving: "Disable anti-aliasing" (`render.opengl_AA_disableOnMove`: a WebGL
 * context's multisampling is fixed when it is made, so the browser cannot turn it off for a moving frame -- the setting is kept and not honoured), "Disable thickness"
 * (`render.opengl_thickness_disableOnMove`), "Disable uVia holes" (`render.opengl_vias_disableOnMove`) and "Disable all plated holes" (`render.opengl_holes_disableOnMove`).
 * "Show model bounding boxes" is the Appearance manager's row of that name.
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
      {heading3d("While Moving")}
      <label className="filter-row">
        <input type="checkbox" aria-label="Disable anti-aliasing" checked={draft.aaDisableOnMove} onChange={(e) => set({ aaDisableOnMove: e.target.checked })} />
        Disable anti-aliasing
      </label>
      <label className="filter-row">
        <input type="checkbox" aria-label="Disable thickness" checked={draft.thicknessDisableOnMove} onChange={(e) => set({ thicknessDisableOnMove: e.target.checked })} />
        Disable thickness
      </label>
      <label className="filter-row">
        <input type="checkbox" aria-label="Disable uVia holes" checked={draft.microviasDisableOnMove} onChange={(e) => set({ microviasDisableOnMove: e.target.checked })} />
        Disable uVia holes
      </label>
      <label className="filter-row">
        <input type="checkbox" aria-label="Disable all plated holes" checked={draft.holesDisableOnMove} onChange={(e) => set({ holesDisableOnMove: e.target.checked })} />
        Disable all plated holes
      </label>
      <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
        <button
          onClick={() => {
            const d = DEFAULT_VIEWER3D_PREFS;
            set({ copperThickness: d.copperThickness, highlightOnRollover: d.highlightOnRollover, antialiasing: d.antialiasing, selectionColor: d.selectionColor, aaDisableOnMove: d.aaDisableOnMove, thicknessDisableOnMove: d.thicknessDisableOnMove, microviasDisableOnMove: d.microviasDisableOnMove, holesDisableOnMove: d.holesDisableOnMove });
          }}
        >
          Reset to Defaults
        </button>
      </div>
      <div style={{ fontSize: 11, opacity: 0.65, marginTop: 10 }}>
        Show model bounding boxes is the Appearance manager&apos;s row. Disable anti-aliasing is kept but not honoured: a browser&apos;s WebGL multisampling is fixed when the view opens.
      </div>
    </div>
  );
}
