// The Objects tab (`APPEARANCE_CONTROLS::rebuildObjects`, rows from `s_objectSettings`): what the board shows besides its layers -- a colour chip where the
// object has a theme colour (a double click edits it in the board editor's theme: themeSwatch.tsx), an eye, the name, and an opacity slider for tracks, vias,
// pads, zones and filled shapes. Footprint Text drags References and Values along with it (`onObjectVisibilityChanged`).
//
// The row (`ObjectRowView`) takes its state and its handlers as props: the Footprint Editor's Appearance manager (components/footprint/FootprintAppearancePanel.tsx)
// draws its rows (`s_allowedInFpEditor`) with it, on its own store.
import { useAppearanceView } from "./useAppearanceView";
import { Eye, Pane, Swatch } from "./shared";
import { useThemeSwatchEdit } from "./themeSwatch";
import { layerColor, layerColorIn } from "../../canvas/layers";
import type { ThemeEditor } from "../../../kicad-port/colorTheme";
import { OBJECT_ROWS, type ObjectId, type ObjectRow } from "../../../kicad-port/appearance";
import { objectChecked } from "../../../kicad-port/appearanceOps";

/** The theme colour an object is drawn in, where it has one of its own (the chip `theme->GetColor( layer )` gives a row); the others are drawn in the layer's colour. */
const COLOR_KEY: Partial<Record<ObjectId, string>> = {
  ratsnest: "LAYER_RATSNEST",
  drc_warnings: "LAYER_DRC_WARNING",
  drc_errors: "LAYER_DRC_ERROR",
  drc_exclusions: "LAYER_DRC_EXCLUSION",
  footprint_anchors: "LAYER_ANCHOR",
  locked_item_shadows: "LAYER_LOCKED_ITEM_SHADOW",
  conflict_shadows: "LAYER_CONFLICTS_SHADOW",
  board_outline_area: "LAYER_BOARD_OUTLINE_AREA",
  drawing_sheet: "LAYER_DRAWINGSHEET",
  grid: "LAYER_GRID",
};

/**
 * One object's row (`rebuildObjects`' `appendObject`): the chip, the eye ("Show or hide <name>", none for a row without a checkbox), the name, and -- for a row with
 * an opacity -- the slider ("Set opacity of <name>"), `opacity` 0..1.
 */
export function ObjectRowView({ row, checked, opacity, onToggle, onOpacity, onEditColor, theme }: { row: ObjectRow & { id: ObjectId }; checked: boolean; opacity?: number; onToggle: () => void; onOpacity?: (value: number) => void; onEditColor?: (colorKey: string) => void; theme?: ThemeEditor }) {
  const id = row.id;
  const colorKey = COLOR_KEY[id];
  return (
    <div className="ap-row" data-object={id} title={row.tooltip}>
      <Swatch
        css={colorKey ? (theme ? layerColorIn(theme, colorKey) : layerColor(colorKey)) : null}
        hidden={!colorKey}
        title={colorKey ? (onEditColor ? "Left double click or middle click for color change, right click for menu" : "Color of this object (set by the color theme)") : undefined}
        onEdit={colorKey && onEditColor ? () => onEditColor(colorKey) : undefined}
      />
      <Eye on={checked} disabled={!row.checkbox} title={`Show or hide ${row.label.toLowerCase()}`} onToggle={onToggle} />
      <span className={row.opacity ? "ap-label-fixed" : "ap-name"}>{row.label}</span>
      {row.opacity && (
        <input
          className="ap-slider grow"
          type="range"
          min={0}
          max={100}
          value={Math.round((opacity ?? 1) * 100)}
          aria-label={`Set opacity of ${row.label.toLowerCase()}`}
          title={`Set opacity of ${row.label.toLowerCase()}`}
          onChange={(e) => onOpacity?.(Number(e.target.value) / 100)}
        />
      )}
    </div>
  );
}

export function ObjectsTab() {
  const { state, op, dispatch } = useAppearanceView();
  // the chips edit the board editor's colour theme (`OnColorSwatchChanged`), with KiCad's default as the dialog's default (`theme->GetDefaultColor( layer )`)
  const theme = useThemeSwatchEdit();
  return (
    <div className="ap-scroll">
      {OBJECT_ROWS.map((row, i) => {
        if (row.id === null) return <div key={`s${i}`} className="ap-spacer" />;
        const id = row.id;
        const checked = objectChecked(state, id);
        return (
          <ObjectRowView
            key={id}
            row={{ ...row, id }}
            checked={checked}
            opacity={row.opacity ? state.appearance.opacity[row.opacity] : undefined}
            onToggle={() => op({ op: "object", id, visible: !checked })}
            onOpacity={(value) => op({ op: "opacity", key: row.opacity!, value })}
            onEditColor={(key) => theme.edit(key, "object")}
          />
        );
      })}
      {/* Not a row of KiCad's Objects tab: Preferences > Display Options "Show pad numbers" (`m_DisplayPadNumbers`, `pcbnew.Control.showPadNumbers`), kept where the studio's panel always had it. */}
      <Pane title="Display Options">
        <label className="ap-check">
          <input type="checkbox" checked={state.bcx.showPadNumbers} onChange={() => dispatch({ type: "BCX", patch: { showPadNumbers: !state.bcx.showPadNumbers } })} />
          Show pad numbers
        </label>
      </Pane>
      {theme.dialog}
    </div>
  );
}
