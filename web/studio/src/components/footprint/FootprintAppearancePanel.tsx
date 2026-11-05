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
// `ObjectRowView`), on the footprint editor's own store. Not here yet: KiCad's Presets and Viewports choosers under the pages (the footprint editor keeps them:
// `FOOTPRINT_EDITOR_SETTINGS::m_LayerPresets`), the colour change of a swatch, and Flip board view (the footprint editor's canvas does not flip).
import { useMemo, useState } from "react";
import { useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { ContrastModeRadios, LayerRowView, layerMenuEntries, type LayerMenuCommand } from "../panels/appearance/LayersTab";
import { ObjectRowView } from "../panels/appearance/ObjectsTab";
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
  const dispatch = useFpDispatch();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const layers = useMemo(() => fpPanelLayers(fpUsedLayers(state.footprint)), [state.footprint]);
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
            layerKey={name}
            label={fpLayerLabel(name)}
            colorKey={isCopper(name) ? copperColorKey(name) : layerStateKey(name)}
            visible={visible}
            active={state.activeLayer === name}
            nameTitle={fpLayerTooltip(name)}
            opacity={state.layerOpacity[name] ?? state.layerOpacity[layerStateKey(name)] ?? 1}
            onActivate={() => dispatch({ type: "SET_ACTIVE_LAYER", layer: name })}
            onToggle={() => dispatch({ type: "SET_LAYERS_VISIBLE", visible: fpLayersPatch([name], !visible) })}
            onOpacity={(value) => setOpacity(name, value)}
          />
        );
      })}
      <Pane title="Layer Display Options">
        <ContrastModeRadios mode={mode} onChange={(m: ContrastMode) => dispatch({ type: "SET_CONTRAST_MODE", mode: m })} />
      </Pane>
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={layerMenuEntries(FP_SHOWS_INNER_LAYERS_ENTRY, run)} onClose={() => setMenu(null)} />}
    </div>
  );
}

function FpObjectsPage() {
  const state = useFpState();
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
            row={row}
            checked={checked}
            opacity={row.opacity ? fpObjectOpacity(state.layerOpacity, row.id) : undefined}
            onToggle={() => toggle(row.id, !checked)}
            onOpacity={(value) => dispatch({ type: "SET_LAYER_OPACITY", key: objectKey(row.id), opacity: value })}
          />
        );
      })}
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
      </div>
    </div>
  );
}
