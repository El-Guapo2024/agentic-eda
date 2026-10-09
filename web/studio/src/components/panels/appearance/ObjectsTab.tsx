// The Objects tab (`APPEARANCE_CONTROLS::rebuildObjects`, rows from `s_objectSettings`): what the board shows besides its layers -- a colour chip where the
// object has a theme colour, an eye, the name, and an opacity slider for tracks, vias, pads, zones and filled shapes. Footprint Text drags References and
// Values along with it (`onObjectVisibilityChanged`).
import { useAppearanceView } from "./useAppearanceView";
import { Eye, Pane, Swatch } from "./shared";
import { layerColor } from "../../canvas/layers";
import { OBJECT_ROWS, type ObjectId } from "../../../kicad-port/appearance";
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

export function ObjectsTab() {
  const { state, op, dispatch } = useAppearanceView();
  return (
    <div className="ap-scroll">
      {OBJECT_ROWS.map((row, i) => {
        if (row.id === null) return <div key={`s${i}`} className="ap-spacer" />;
        const id = row.id;
        const checked = objectChecked(state, id);
        const colorKey = COLOR_KEY[id];
        return (
          <div className="ap-row" key={id} data-object={id} title={row.tooltip}>
            <Swatch css={colorKey ? layerColor(colorKey) : null} hidden={!colorKey} title={colorKey ? "Color of this object (set by the color theme)" : undefined} />
            <Eye on={checked} disabled={!row.checkbox} title={`Show or hide ${row.label.toLowerCase()}`} onToggle={() => op({ op: "object", id, visible: !checked })} />
            <span className={row.opacity ? "ap-label-fixed" : "ap-name"}>{row.label}</span>
            {row.opacity && (
              <input
                className="ap-slider grow"
                type="range"
                min={0}
                max={100}
                value={Math.round(state.appearance.opacity[row.opacity] * 100)}
                aria-label={`Set opacity of ${row.label.toLowerCase()}`}
                title={`Set opacity of ${row.label.toLowerCase()}`}
                onChange={(e) => op({ op: "opacity", key: row.opacity!, value: Number(e.target.value) / 100 })}
              />
            )}
          </div>
        );
      })}
      {/* Not a row of KiCad's Objects tab: Preferences > Display Options "Show pad numbers" (`m_DisplayPadNumbers`, `pcbnew.Control.showPadNumbers`), kept where the studio's panel always had it. */}
      <Pane title="Display Options">
        <label className="ap-check">
          <input type="checkbox" checked={state.bcx.showPadNumbers} onChange={() => dispatch({ type: "BCX", patch: { showPadNumbers: !state.bcx.showPadNumbers } })} />
          Show pad numbers
        </label>
      </Pane>
    </div>
  );
}
