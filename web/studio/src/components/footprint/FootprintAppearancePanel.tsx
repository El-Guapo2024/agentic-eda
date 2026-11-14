// The Footprint Editor's Appearance manager (UI-LEFT F2): `APPEARANCE_CONTROLS` built for the footprint editor (pcbnew/widgets/appearance_controls.cpp, `m_isFpEditor`), the
// "LayersManager" pane of footprint_edit_frame.cpp, captioned "Appearance" and docked at the right of the frame outside the drawing toolbar (`.Right().Layer( 3 )` against the
// toolbar's `.Right().Layer( 2 )`), shown and hidden by `pcbnew.Control.showLayersManager` (`FOOTPRINT_EDITOR_CONTROL::ToggleLayersManager`, the store's `appearanceShown`).
//
// Two pages, KiCad's Layers and Objects (the footprint editor removes the Nets page: `m_notebook->RemovePage( 2 )`):
//   Layers   the editor's board's layers (kicad-port/fpAppearance.ts `fpPanelLayers`: F.Cu, "Inner layers", B.Cu, the technical layers, User.1 .. User.4 and any other user
//            layer the footprint uses), each with the active-layer bar, its colour (read-only, as on the board), the eye and its name; a click makes it the active layer the
//            drawing tools draw on (`onLayerLeftClick`: `m_frame->SetActiveLayer( layer )`), a right click opens the layer menu (`rebuildLayerContextMenu`). Layer Display
//            Options: Inactive layers Normal / Dim / Hide (the store's `highContrast` and `contrastHidden`).
//   Objects  the rows of `s_allowedInFpEditor`, each with its eye and, for pads, zones, filled shapes, images, tracks and vias, its opacity slider.
//
// The rows and the menu are the board panel's (components/panels/appearance/LayersTab.tsx `LayerRowView`, `layerMenuEntries`, `ContrastModeRadios`; ObjectsTab.tsx
// `ObjectRowView`), on the footprint editor's own store. Under the pages, KiCad's Presets and Viewports lists (the board's, components/panels/appearance/PresetBar.tsx,
// on this editor: the person's presets kept in its preferences, `FOOTPRINT_EDITOR_SETTINGS::m_LayerPresets`; the viewports for the session, which KiCad's Footprint Editor
// does not save). Below the manager the Selection Filter pane (`m_selectionFilterPanel`, the "SelectionFilter" pane under "LayersManager"), its filter kept in the
// preferences (`m_SelectionFilter`). A swatch edits the Footprint Editor's own colour theme (`useThemeSwatchEdit`, as the board's does its own). UI-LEFT-2 FF5. Layer
// Display Options' "Flip board view" (`m_cbFlipBoard`: `RunAction( PCB_ACTIONS::flipBoard )`) and the presets' flip, UI-LEFT-3 F3-9.
import { useMemo, useState } from "react";
import { useStudioDispatch } from "../../state/store";
import { MessageDialog, NameDialog, PickDialog } from "../panels/appearance/shared";
import { SelectionFilterView } from "../panels/SelectionFilterPanel";
import { allPresets, currentViewport, presetList, savePresetOutcome, selectPreset, snapshotPreset, viewForViewport, withPreset, withoutPreset, withViewport, withoutViewport } from "../../kicad-port/layerPresets";
import { MAX_SCALE, MIN_SCALE } from "../../kicad-port/view";
import { fpApplyPresetView, fpMatchingPreset, fpPresetView } from "../../kicad-port/fpAppearance";
import { useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { useColorThemeStamp } from "../../state/colorThemeStore";
import { ContrastModeRadios, LayerRowView, layerMenuEntries, type LayerMenuCommand } from "../panels/appearance/LayersTab";
import { ObjectRowView } from "../panels/appearance/ObjectsTab";
import { useThemeSwatchEdit } from "../panels/appearance/themeSwatch";
import { Pane } from "../panels/appearance/shared";
import { ContextMenu } from "../canvas/ContextMenu";
import { copperColorKey } from "../canvas/layers";
import { objectKey, type ContrastMode, type ObjectId } from "../../kicad-port/appearance";
import { isCopper, layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";
import {
  FP_OBJECT_ROWS,
  FP_SHOWS_INNER_LAYERS_ENTRY,
  fpContrastMode,
  fpLayerLabel,
  fpLayerMenu,
  fpLayersPatch,
  fpLayerTooltip,
  fpObjectChecked,
  fpObjectOpacity,
  fpObjectPatch,
  fpPanelLayers,
  fpUsedLayers,
} from "../../kicad-port/fpAppearance";
import "../../styles/appearance.css";

type Page = "layers" | "objects";

const PAGES: ReadonlyArray<{ id: Page; label: string }> = [
  { id: "layers", label: "Layers" },
  { id: "objects", label: "Objects" },
];

function FpLayersPage() {
  const state = useFpState();
  useColorThemeStamp(); // the swatches are the Footprint Editor's theme's (Preferences > Footprint Editor > Colors)
  const dispatch = useFpDispatch();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const swatch = useThemeSwatchEdit("footprint");
  // the user layers of Preferences > Footprint Editor > User Layer Names: so many of them, every named one, shown by its name (UI-LEFT-2 FF7)
  const { fpUserLayerCount, fpUserLayerNames } = useStudioState().prefs;
  const layers = useMemo(() => fpPanelLayers([...fpUsedLayers(state.footprint), ...Object.keys(fpUserLayerNames)], fpUserLayerCount), [state.footprint, fpUserLayerCount, fpUserLayerNames]);
  const mode = fpContrastMode(state.highContrast, state.contrastHidden);
  const run = (cmd: LayerMenuCommand) => {
    const next = fpLayerMenu(layers, state.layerVisible, state.activeLayer, cmd);
    dispatch({ type: "SET_LAYERS_VISIBLE", visible: next.visible });
    if (next.activeLayer !== state.activeLayer) dispatch({ type: "SET_ACTIVE_LAYER", layer: next.activeLayer });
  };
  // The painter reads a graphic's opacity by its layer's name, the board's painters (fields, text boxes, dimensions ...) by its state key: the slider sets both.
  const setOpacity = (name: string, value: number) => {
    dispatch({ type: "SET_LAYER_OPACITY", key: name, opacity: value });
    if (layerStateKey(name) !== name) dispatch({ type: "SET_LAYER_OPACITY", key: layerStateKey(name), opacity: value });
  };
  return (
    <div className="ap-scroll" onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY }); }}>
      {layers.map((name) => {
        const visible = layerIsVisible(state.layerVisible, name);
        return (
          <LayerRowView
            key={name}
            theme="footprint"
            layerKey={name}
            label={fpLayerLabel(name, fpUserLayerNames)}
            colorKey={isCopper(name) ? copperColorKey(name) : layerStateKey(name)}
            visible={visible}
            active={state.activeLayer === name}
            nameTitle={fpLayerTooltip(name)}
            opacity={state.layerOpacity[name] ?? state.layerOpacity[layerStateKey(name)] ?? 1}
            onActivate={() => dispatch({ type: "SET_ACTIVE_LAYER", layer: name })}
            onToggle={() => dispatch({ type: "SET_LAYERS_VISIBLE", visible: fpLayersPatch([name], !visible) })}
            onOpacity={(value) => setOpacity(name, value)}
            onEditColor={() => swatch.edit(isCopper(name) ? copperColorKey(name) : layerStateKey(name), "layer")}
          />
        );
      })}
      {swatch.dialog}
      <Pane title="Layer Display Options">
        <ContrastModeRadios mode={mode} onChange={(m: ContrastMode) => dispatch({ type: "SET_CONTRAST_MODE", mode: m })} />
        {/* `m_cbFlipBoard`: the same switch as View > Flip Board View (`RunAction( PCB_ACTIONS::flipBoard )`) */}
        <label className="ap-check">
          <input type="checkbox" data-testid="fp-flip-board" checked={state.boardFlipped} onChange={(e) => dispatch({ type: "SET_DISPLAY", patch: { boardFlipped: e.target.checked } })} />
          Flip board view
        </label>
      </Pane>
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={layerMenuEntries(FP_SHOWS_INNER_LAYERS_ENTRY, run)} onClose={() => setMenu(null)} />}
    </div>
  );
}

function FpObjectsPage() {
  const state = useFpState();
  const swatch = useThemeSwatchEdit("footprint");
  useColorThemeStamp(); // the swatches are the Footprint Editor's theme's (Preferences > Footprint Editor > Colors)
  const dispatch = useFpDispatch();
  const toggle = (id: ObjectId, on: boolean) => {
    // `case LAYER_GRID: m_frame->SetGridVisibility( isVisible )`: the row is the frame's grid switch (View > Show Grid), so the toolbar's button follows it.
    if (id === "grid") {
      if (on) dispatch({ type: "SET_LAYER_VISIBLE", key: objectKey("grid"), visible: true });
      if (state.gridVisible !== on) dispatch({ type: "TOGGLE_GRID_VISIBLE" });
      return;
    }
    dispatch({ type: "SET_LAYERS_VISIBLE", visible: fpObjectPatch(state.layerVisible, id, on) });
  };
  return (
    <div className="ap-scroll">
      {FP_OBJECT_ROWS.map((row) => {
        const checked = fpObjectChecked(state, row.id);
        return (
          <ObjectRowView
            key={row.id}
            theme="footprint"
            row={row}
            checked={checked}
            opacity={row.opacity ? fpObjectOpacity(state.layerOpacity, row.id) : undefined}
            onToggle={() => toggle(row.id, !checked)}
            onOpacity={(value) => dispatch({ type: "SET_LAYER_OPACITY", key: objectKey(row.id), opacity: value })}
            onEditColor={(key) => swatch.edit(key, "object")}
          />
        );
      })}
      {swatch.dialog}
    </div>
  );
}

/** The panel's layers (`fpPanelLayers`), as the Layers page lists them. */
function useFpPanelLayers(): string[] {
  const state = useFpState();
  const { fpUserLayerCount, fpUserLayerNames } = useStudioState().prefs;
  return useMemo(() => fpPanelLayers([...fpUsedLayers(state.footprint), ...Object.keys(fpUserLayerNames)], fpUserLayerCount), [state.footprint, fpUserLayerCount, fpUserLayerNames]);
}

const SAVE = "cmd:save";
const DELETE = "cmd:delete";
type Dlg = null | { kind: "save_preset" } | { kind: "overwrite"; name: string } | { kind: "refused"; message: string } | { kind: "delete_preset" } | { kind: "save_viewport" } | { kind: "delete_viewport" };

/**
 * `m_cbLayerPresets` and `m_cbViewports` in the Footprint Editor: the built-in presets and the person's (`FOOTPRINT_EDITOR_SETTINGS::m_LayerPresets`), "Save preset..." and
 * "Delete preset..." (`onLayerPresetChanged`, KiCad's dialogs and messages), the list showing the preset that matches what is on screen (`syncLayerPresetSelection`); the
 * viewports of the session with "Save viewport..." and "Delete viewport..." (`onViewportChanged`: `VIEW::SetViewport`).
 */
function FpPresetBar() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const studioDispatch = useStudioDispatch();
  const mine = useStudioState().prefs.fpLayerPresets;
  const universe = useFpPanelLayers();
  const [dlg, setDlg] = useState<Dlg>(null);
  const [viewport, setViewport] = useState("");
  const copper = universe.filter((l) => isCopper(l));
  const optional = universe.filter((l) => !isCopper(l));
  const everyone = allPresets(copper, mine, optional);
  const view = fpPresetView(universe, state);
  const active = fpMatchingPreset(everyone, view);
  const canvasSize = () => {
    const rect = document.querySelector("[data-fp-canvas]")?.getBoundingClientRect();
    return rect && rect.width > 0 && rect.height > 0 ? { w: rect.width, h: rect.height } : null;
  };
  const apply = (name: string) => {
    const preset = everyone.find((p) => p.name === name);
    if (!preset) return;
    const next = selectPreset(fpPresetView(universe, state, active, state.lastBuiltinObjects), preset, everyone);
    const out = fpApplyPresetView(universe, state.layerVisible, next);
    dispatch({ type: "SET_LAYERS_VISIBLE", visible: out.visible });
    if (out.grid !== state.gridVisible) dispatch({ type: "TOGGLE_GRID_VISIBLE" });
    if (out.activeLayer && out.activeLayer !== state.activeLayer) dispatch({ type: "SET_ACTIVE_LAYER", layer: out.activeLayer });
    // `options.m_FlipBoardView = aPreset.flipBoard; m_frame->SetDisplayOptions( options, false )`
    if (out.flipped !== state.boardFlipped) dispatch({ type: "SET_DISPLAY", patch: { boardFlipped: out.flipped } });
    dispatch({ type: "SET_LAST_BUILTIN_OBJECTS", objects: next.lastBuiltinObjects ? [...next.lastBuiltinObjects] : null });
  };
  const setMine = (fpLayerPresets: typeof mine) => studioDispatch({ type: "PATCH_PREFERENCES", patch: { fpLayerPresets } });
  const savePreset = (name: string) => {
    const outcome = savePresetOutcome(name, everyone);
    if (outcome.kind === "overwrite") return setDlg({ kind: "overwrite", name });
    if (outcome.kind === "refused") return setDlg({ kind: "refused", message: outcome.message });
    if (outcome.kind === "new") setMine(withPreset(mine, snapshotPreset(name, view)));
    setDlg(null);
  };
  return (
    <div className="ap-footer">
      <div className="ap-combo-row">
        <label htmlFor="fp-ap-presets">Presets:</label>
        <select
          id="fp-ap-presets"
          className="ap-combo"
          data-testid="fp-presets"
          title="Save and restore layer visibility combinations."
          value={active === "" ? "" : `p:${active}`}
          onChange={(e) => {
            const v = e.target.value;
            if (v === SAVE) setDlg({ kind: "save_preset" });
            else if (v === DELETE) setDlg({ kind: "delete_preset" });
            else if (v.startsWith("p:")) apply(v.slice(2));
          }}
        >
          {active === "" && <option value="" disabled></option>}
          {presetList(copper, mine, optional).map((e, i) =>
            e.kind === "preset" ? (
              <option key={e.preset.name} value={`p:${e.preset.name}`}>
                {e.preset.name}
              </option>
            ) : e.kind === "separator" ? (
              <option key={`s${i}`} value={`sep${i}`} disabled>
                ---
              </option>
            ) : e.kind === "save" ? (
              <option key="save" value={SAVE}>
                Save preset...
              </option>
            ) : (
              <option key="delete" value={DELETE}>
                Delete preset...
              </option>
            )
          )}
        </select>
      </div>
      <div className="ap-combo-row">
        <label htmlFor="fp-ap-viewports">Viewports:</label>
        <select
          id="fp-ap-viewports"
          className="ap-combo"
          data-testid="fp-viewports"
          title="Save and restore view location and zoom."
          value={viewport === "" ? "" : `v:${viewport}`}
          onChange={(e) => {
            const v = e.target.value;
            if (v === SAVE) setDlg({ kind: "save_viewport" });
            else if (v === DELETE) setDlg({ kind: "delete_viewport" });
            else if (v.startsWith("v:")) {
              const vp = state.viewports.find((x) => x.name === v.slice(2));
              const size = canvasSize();
              if (!vp || !size) return;
              setViewport(vp.name);
              const next = viewForViewport(vp, size.w, size.h);
              dispatch({ type: "SET_VIEW", view: { ...next, scale: Math.max(MIN_SCALE, Math.min(MAX_SCALE, next.scale)) } });
            }
          }}
        >
          {viewport === "" && <option value="" disabled></option>}
          {state.viewports.map((v) => (
            <option key={v.name} value={`v:${v.name}`}>
              {v.name}
            </option>
          ))}
          <option value="sep" disabled>
            ---
          </option>
          <option value={SAVE}>Save viewport...</option>
          <option value={DELETE}>Delete viewport...</option>
        </select>
      </div>
      {dlg?.kind === "save_preset" && <NameDialog title="Save Layer Preset" label="Layer preset name:" initial={mine.some((p) => p.name === active) ? active : ""} onCancel={() => setDlg(null)} onOk={savePreset} />}
      {dlg?.kind === "overwrite" && (
        <MessageDialog
          title="Save Layer Preset"
          message="Overwrite existing preset?"
          onCancel={() => setDlg(null)}
          onOk={() => {
            setMine(withPreset(mine, snapshotPreset(dlg.name, view)));
            setDlg(null);
          }}
        />
      )}
      {dlg?.kind === "refused" && <MessageDialog title="Error" message={dlg.message} onCancel={() => setDlg(null)} onOk={() => setDlg(null)} />}
      {dlg?.kind === "delete_preset" && (
        <PickDialog
          title="Delete Preset"
          header="Presets"
          label="Select preset:"
          items={mine.map((p) => p.name)}
          onCancel={() => setDlg(null)}
          onOk={(name) => {
            setMine(withoutPreset(mine, name));
            setDlg(null);
          }}
        />
      )}
      {dlg?.kind === "save_viewport" && (
        <NameDialog
          title="Save Viewport"
          label="Viewport name:"
          onCancel={() => setDlg(null)}
          onOk={(name) => {
            const size = canvasSize();
            if (size && name.trim() !== "") {
              dispatch({ type: "SET_VIEWPORTS", viewports: withViewport(state.viewports, name.trim(), currentViewport(state.view, size.w, size.h)) });
              setViewport(name.trim());
            }
            setDlg(null);
          }}
        />
      )}
      {dlg?.kind === "delete_viewport" && (
        <PickDialog
          title="Delete Viewport"
          header="Viewports"
          label="Select viewport:"
          items={state.viewports.map((v) => v.name)}
          onCancel={() => setDlg(null)}
          onOk={(name) => {
            dispatch({ type: "SET_VIEWPORTS", viewports: withoutViewport(state.viewports, name) });
            if (viewport === name) setViewport("");
            setDlg(null);
          }}
        />
      )}
    </div>
  );
}

/** The Footprint Editor's Selection Filter pane (`PANEL_SELECTION_FILTER`, `FOOTPRINT_EDITOR_SETTINGS::m_SelectionFilter` kept in the preferences). */
function FpSelectionFilterPane() {
  const filter = useStudioState().prefs.fpSelectionFilter;
  const studioDispatch = useStudioDispatch();
  return (
    <div className="fp-selection-filter" data-testid="fp-selection-filter">
      <SelectionFilterView filter={filter} onChange={(patch) => studioDispatch({ type: "PATCH_PREFERENCES", patch: { fpSelectionFilter: { ...filter, ...(patch as Partial<typeof filter>) } } })} />
    </div>
  );
}

export function FootprintAppearancePanel() {
  const [page, setPage] = useState<Page>("layers");
  return (
    <div className="dock fp-appearance" data-testid="fp-appearance" aria-label="Appearance">
      <header className="dock-panel-caption">
        <span className="dock-panel-title">Appearance</span>
      </header>
      <div className="ap-root" style={{ flex: 1, minHeight: 0 }}>
        <div className="dock-tabs">
          {PAGES.map((p) => (
            <div key={p.id} className={`dock-tab${page === p.id ? " active" : ""}`} data-tab={p.id} onClick={() => setPage(p.id)}>
              {p.label}
            </div>
          ))}
        </div>
        <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
          {page === "layers" && <FpLayersPage />}
          {page === "objects" && <FpObjectsPage />}
        </div>
        <FpPresetBar />
      </div>
      <FpSelectionFilterPane />
    </div>
  );
}
