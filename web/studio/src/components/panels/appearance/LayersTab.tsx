// The Layers tab (`APPEARANCE_CONTROLS::rebuildLayers`): the board's copper layers front to back, then the technical and user layers it has (Board Editor Layers), each with the active-layer
// bar, its colour, an eye and its name; a click on a row makes it the active layer, a right click anywhere in the list opens the layer menu
// (`rebuildLayerContextMenu`). Below: "Layer Display Options", the inactive-layer mode and Flip board view. (The presets and viewports are in the panel's footer.)
//
// The row (`LayerRowView`), the menu's entries (`layerMenuEntries`) and the inactive-layer radios (`ContrastModeRadios`) take what they show and do as props: the Footprint Editor's
// Appearance manager (components/footprint/FootprintAppearancePanel.tsx) draws its own layers with them, on its own store.
import { useState } from "react";
import { useAppearanceView } from "./useAppearanceView";
import { Eye, Pane, Radios, Swatch } from "./shared";
import { useThemeSwatchEdit } from "./themeSwatch";
import { ContextMenu, type MenuEntry } from "../../canvas/ContextMenu";
import { copperColorKey, layerColor, layerColorIn } from "../../canvas/layers";
import type { ThemeEditor } from "../../../kicad-port/colorTheme";
import { layerStateKey, type LayerGroupOp, type MenuPresetKind } from "../../../kicad-port/layerPresets";
import { enabledOptionalLayers, layerDisplayName } from "../../../kicad-port/boardLayers";
import type { ContrastMode } from "../../../kicad-port/appearance";

/**
 * One layer's row (`rebuildLayers`' `appendLayer`): the active-layer indicator, the colour (the theme's; with `onEditColor` a double click or a middle click edits it,
 * "Double click or middle click for color change, right click for menu"), the eye ("Show or hide this layer"), the name, and the studio's own opacity slider when
 * `onOpacity` is given. `layerKey` is the row's `data-layer`.
 */
export function LayerRowView({
  layerKey,
  label,
  colorKey,
  visible,
  active,
  nameTitle,
  opacity,
  onActivate,
  onToggle,
  onOpacity,
  onEditColor,
  theme,
}: {
  layerKey: string;
  label: string;
  colorKey: string;
  visible: boolean;
  active: boolean;
  nameTitle?: string;
  opacity?: number;
  onActivate: () => void;
  onToggle: () => void;
  onOpacity?: (value: number) => void;
  onEditColor?: () => void;
  /** The editor whose colour theme the swatch shows (the board editor's when not given). */
  theme?: ThemeEditor;
}) {
  return (
    <div className={`ap-row${active ? " active" : ""}`} data-layer={layerKey} onClick={onActivate}>
      <span className="ap-indicator" />
      <Swatch css={theme ? layerColorIn(theme, colorKey) : layerColor(colorKey)} title={onEditColor ? "Double click or middle click for color change, right click for menu" : "Layer color"} onEdit={onEditColor} />
      <Eye on={visible} title="Show or hide this layer" onToggle={onToggle} />
      <span className="ap-name" title={nameTitle}>
        {label}
      </span>
      {onOpacity && <input className="ap-slider" type="range" min={0} max={1} step={0.05} value={opacity ?? 1} aria-label={`Opacity of ${label}`} onClick={(e) => e.stopPropagation()} onChange={(e) => onOpacity(parseFloat(e.target.value))} />}
    </div>
  );
}

function LayerRow({ stateKey, label, colorKey, onEditColor }: { stateKey: string; label: string; colorKey: string; onEditColor: () => void }) {
  const { state, dispatch, op } = useAppearanceView();
  const visible = state.layerVisible[stateKey] !== false;
  const opacity = state.layerOpacity[stateKey] ?? 1;
  const active = state.activeLayer === stateKey;
  return (
    <LayerRowView
      layerKey={stateKey}
      label={label}
      colorKey={colorKey}
      visible={visible}
      active={active}
      nameTitle="Click to make this the active layer"
      opacity={opacity}
      onActivate={() => dispatch({ type: "SET_ACTIVE_LAYER", layer: active ? null : stateKey })}
      onToggle={() => op({ op: "layer", key: stateKey, visible: !visible })}
      onOpacity={(value) => op({ op: "layer_opacity", key: stateKey, value })}
      onEditColor={onEditColor}
    />
  );
}

/** What an entry of the layer menu does (`OnLayerContextMenu`'s ids): a copper / non-copper group, Hide All Layers But Active, or one of the built-in presets' layers. */
export type LayerMenuCommand = LayerGroupOp | "hide_all_but_active" | MenuPresetKind;

/** `rebuildLayerContextMenu`'s entries, in its order; "Show Only Inner Layers" only when `innerLayers` ("Only show the internal layer option if internal layers are enabled"). */
export function layerMenuEntries(innerLayers: boolean, run: (cmd: LayerMenuCommand) => void): MenuEntry[] {
  const e: MenuEntry[] = [
    { label: "Show All Copper Layers", onSelect: () => run("show_copper") },
    { label: "Hide All Copper Layers", onSelect: () => run("hide_copper") },
    { label: "", separator: true, onSelect: () => {} },
    { label: "Hide All Layers But Active", onSelect: () => run("hide_all_but_active") },
    { label: "", separator: true, onSelect: () => {} },
    { label: "Show All Non Copper Layers", onSelect: () => run("show_non_copper") },
    { label: "Hide All Non Copper Layers", onSelect: () => run("hide_non_copper") },
    { label: "", separator: true, onSelect: () => {} },
    { label: "Show All Layers", onSelect: () => run("all_layers") },
    { label: "Hide All Layers", onSelect: () => run("no_layers") },
    { label: "", separator: true, onSelect: () => {} },
    { label: "Show Only Front Assembly Layers", onSelect: () => run("front_assembly") },
    { label: "Show Only Front Layers", onSelect: () => run("front") },
  ];
  if (innerLayers) e.push({ label: "Show Only Inner Layers", onSelect: () => run("inner_copper") });
  e.push({ label: "Show Only Back Layers", onSelect: () => run("back") }, { label: "Show Only Back Assembly Layers", onSelect: () => run("back_assembly") });
  return e;
}

/** "Inactive layers (H):" with Normal / Dim / Hide (`m_rbHighContrastNormal` ...), in the Layer Display Options pane. */
export function ContrastModeRadios({ mode, onChange }: { mode: ContrastMode; onChange: (mode: ContrastMode) => void }) {
  return (
    <>
      <div className="ap-sub">Inactive layers (H):</div>
      <Radios<ContrastMode>
        name="Inactive layers"
        value={mode}
        onChange={onChange}
        options={[
          { value: "normal", label: "Normal", title: "Inactive layers will be shown in full color" },
          { value: "dimmed", label: "Dim", title: "Inactive layers will be dimmed" },
          { value: "hidden", label: "Hide", title: "Inactive layers will be hidden" },
        ]}
      />
    </>
  );
}

const GROUP_OPS: readonly LayerMenuCommand[] = ["show_copper", "hide_copper", "show_non_copper", "hide_non_copper"];

export function LayersTab() {
  const { state, op, nets } = useAppearanceView();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  // the swatches edit the board editor's colour theme (`OnColorSwatchChanged`)
  const theme = useThemeSwatchEdit();
  const board = state.board;
  if (!board) return <div className="ap-note">No board.</div>;
  // Board Setup > Board Editor Layers: the layers the board has and the names the user gave them (`BOARD::GetLayerName`).
  const setup = board.board_rules?.layer_setup;
  const mode: ContrastMode = !state.highContrast ? "normal" : state.appearance.contrastHidden ? "hidden" : "dimmed";
  const run = (cmd: LayerMenuCommand) => {
    if (cmd === "hide_all_but_active") op({ op: "hide_all_but_active" });
    else if (GROUP_OPS.includes(cmd)) op({ op: "layer_group", group: cmd as LayerGroupOp });
    else op({ op: "menu_preset", kind: cmd as MenuPresetKind });
  };
  void nets;
  return (
    <div className="ap-scroll" onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY }); }}>
      {board.layers.map((layer) => (
        <LayerRow key={layer} stateKey={layer} label={layerDisplayName(setup, layer)} colorKey={copperColorKey(layer)} onEditColor={() => theme.edit(copperColorKey(layer), "layer")} />
      ))}
      {enabledOptionalLayers(setup).map((name) => (
        <LayerRow key={name} stateKey={layerStateKey(name)} label={layerDisplayName(setup, name)} colorKey={layerStateKey(name)} onEditColor={() => theme.edit(layerStateKey(name), "layer")} />
      ))}
      <Pane title="Layer Display Options">
        <ContrastModeRadios mode={mode} onChange={(m) => op({ op: "contrast", mode: m })} />
        {/* `m_cbFlipBoard`: the same switch as View > Flip Board View. */}
        <label className="ap-check">
          <input type="checkbox" checked={state.bcx.boardFlipped} onChange={(e) => op({ op: "flip", flipped: e.target.checked })} />
          Flip board view
        </label>
      </Pane>
      {/* `board.layers.length > 2`: "Only show the internal layer option if internal layers are enabled" */}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={layerMenuEntries(board.layers.length > 2, run)} onClose={() => setMenu(null)} />}
      {theme.dialog}
    </div>
  );
}
