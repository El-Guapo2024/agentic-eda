// The Layers tab (`APPEARANCE_CONTROLS::rebuildLayers`): the board's copper layers front to back, then the technical layers, each with the active-layer
// bar, its colour, an eye and its name; a click on a row makes it the active layer, a right click anywhere in the list opens the layer menu
// (`rebuildLayerContextMenu`). Below: "Layer Display Options", the inactive-layer mode and Flip board view. (The presets and viewports are in the panel's footer.)
import { useState } from "react";
import { useAppearanceView } from "./useAppearanceView";
import { Eye, Pane, Radios, Swatch } from "./shared";
import { ContextMenu, type MenuEntry } from "../../canvas/ContextMenu";
import { copperColorKey, layerColor } from "../../canvas/layers";
import { TECH_LAYER_NAMES, layerStateKey } from "../../../kicad-port/layerPresets";
import type { ContrastMode } from "../../../kicad-port/appearance";

function LayerRow({ stateKey, label, colorKey }: { stateKey: string; label: string; colorKey: string }) {
  const { state, dispatch, op } = useAppearanceView();
  const visible = state.layerVisible[stateKey] !== false;
  const opacity = state.layerOpacity[stateKey] ?? 1;
  const active = state.activeLayer === stateKey;
  return (
    <div className={`ap-row${active ? " active" : ""}`} data-layer={stateKey} onClick={() => dispatch({ type: "SET_ACTIVE_LAYER", layer: active ? null : stateKey })}>
      <span className="ap-indicator" />
      <Swatch css={layerColor(colorKey)} title="Layer color" />
      <Eye on={visible} title="Show or hide this layer" onToggle={() => op({ op: "layer", key: stateKey, visible: !visible })} />
      <span className="ap-name" title="Click to make this the active layer">
        {label}
      </span>
      <input className="ap-slider" type="range" min={0} max={1} step={0.05} value={opacity} aria-label={`Opacity of ${label}`} onClick={(e) => e.stopPropagation()} onChange={(e) => op({ op: "layer_opacity", key: stateKey, value: parseFloat(e.target.value) })} />
    </div>
  );
}

export function LayersTab() {
  const { state, op, nets } = useAppearanceView();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const board = state.board;
  if (!board) return <div className="ap-note">No board.</div>;
  const mode: ContrastMode = !state.highContrast ? "normal" : state.appearance.contrastHidden ? "hidden" : "dimmed";
  const entries = (): MenuEntry[] => {
    const e: MenuEntry[] = [
      { label: "Show All Copper Layers", onSelect: () => op({ op: "layer_group", group: "show_copper" }) },
      { label: "Hide All Copper Layers", onSelect: () => op({ op: "layer_group", group: "hide_copper" }) },
      { label: "", separator: true, onSelect: () => {} },
      { label: "Hide All Layers But Active", onSelect: () => op({ op: "hide_all_but_active" }) },
      { label: "", separator: true, onSelect: () => {} },
      { label: "Show All Non Copper Layers", onSelect: () => op({ op: "layer_group", group: "show_non_copper" }) },
      { label: "Hide All Non Copper Layers", onSelect: () => op({ op: "layer_group", group: "hide_non_copper" }) },
      { label: "", separator: true, onSelect: () => {} },
      { label: "Show All Layers", onSelect: () => op({ op: "menu_preset", kind: "all_layers" }) },
      { label: "Hide All Layers", onSelect: () => op({ op: "menu_preset", kind: "no_layers" }) },
      { label: "", separator: true, onSelect: () => {} },
      { label: "Show Only Front Assembly Layers", onSelect: () => op({ op: "menu_preset", kind: "front_assembly" }) },
      { label: "Show Only Front Layers", onSelect: () => op({ op: "menu_preset", kind: "front" }) },
    ];
    // "Only show the internal layer option if internal layers are enabled"
    if (board.layers.length > 2) e.push({ label: "Show Only Inner Layers", onSelect: () => op({ op: "menu_preset", kind: "inner_copper" }) });
    e.push({ label: "Show Only Back Layers", onSelect: () => op({ op: "menu_preset", kind: "back" }) }, { label: "Show Only Back Assembly Layers", onSelect: () => op({ op: "menu_preset", kind: "back_assembly" }) });
    return e;
  };
  void nets;
  return (
    <div className="ap-scroll" onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY }); }}>
      {board.layers.map((layer) => (
        <LayerRow key={layer} stateKey={layer} label={layer} colorKey={copperColorKey(layer)} />
      ))}
      {TECH_LAYER_NAMES.map((name) => (
        <LayerRow key={name} stateKey={layerStateKey(name)} label={name} colorKey={layerStateKey(name)} />
      ))}
      <Pane title="Layer Display Options">
        <div className="ap-sub">Inactive layers (H):</div>
        <Radios<ContrastMode>
          name="Inactive layers"
          value={mode}
          onChange={(m) => op({ op: "contrast", mode: m })}
          options={[
            { value: "normal", label: "Normal", title: "Inactive layers will be shown in full color" },
            { value: "dimmed", label: "Dim", title: "Inactive layers will be dimmed" },
            { value: "hidden", label: "Hide", title: "Inactive layers will be hidden" },
          ]}
        />
        {/* `m_cbFlipBoard`: the same switch as View > Flip Board View. */}
        <label className="ap-check">
          <input type="checkbox" checked={state.bcx.boardFlipped} onChange={(e) => op({ op: "flip", flipped: e.target.checked })} />
          Flip board view
        </label>
      </Pane>
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={entries()} onClose={() => setMenu(null)} />}
    </div>
  );
}
