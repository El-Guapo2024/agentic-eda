// The combo boxes of the Footprint Editor's and the Symbol Editor's top toolbars (`ACTION_TOOLBAR_CONTROLS`: the grid, zoom and layer selectors of
// toolbars_footprint_editor.cpp, the symbol body style and unit selectors of toolbars_symbol_editor.cpp), bound to each editor's own store. The board
// editor's controls are in Toolbar.tsx, bound to the board's.
import { DEFAULT_PCB_GRIDS_UM } from "../../kicad-port/grid";
import { zoomAbout } from "../../kicad-port/view";
import { useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { formatLength } from "../../state/units";

/** The layers the footprint editor's graphics and text tools draw on (the Layer selector's entries; its tools place silkscreen, fab and courtyard items). */
const GRAPHIC_LAYERS = ["F.SilkS", "F.Fab", "F.CrtYd"];

/** 100% is one screen pixel per 100 um, the same reference the board toolbar's zoom box uses. */
const ZOOM_PRESET_PERCENTS = [25, 50, 100, 200, 400, 800, 1600, 3200];
const PERCENT_OF_SCALE = 100 / 0.01;

function editorCanvasRect(): DOMRect | null {
  return document.querySelector(".editor-canvas-col .pcb-canvas-container")?.getBoundingClientRect() ?? null;
}

function GridSelect({ gridUm, onChange }: { gridUm: number; onChange: (um: number) => void }) {
  const { units } = useStudioState();
  const options = DEFAULT_PCB_GRIDS_UM.includes(gridUm) ? DEFAULT_PCB_GRIDS_UM : [...DEFAULT_PCB_GRIDS_UM, gridUm];
  return (
    <div className="toolbar-control" title="Grid">
      <select value={gridUm} onChange={(e) => onChange(Number(e.target.value))}>
        {options.map((um) => (
          <option key={um} value={um}>
            {formatLength(um, units)}
          </option>
        ))}
      </select>
    </div>
  );
}

function ZoomSelect({ view, onView }: { view: { scale: number; x: number; y: number }; onView: (v: { scale: number; x: number; y: number }) => void }) {
  const current = Math.round(view.scale * PERCENT_OF_SCALE);
  return (
    <div className="toolbar-control" title="Zoom">
      <select
        value={ZOOM_PRESET_PERCENTS.includes(current) ? current : ""}
        onChange={(e) => {
          const rect = editorCanvasRect();
          if (!rect || !(view.scale > 0)) return;
          const factor = Number(e.target.value) / PERCENT_OF_SCALE / view.scale;
          onView(zoomAbout(view, rect.width / 2, rect.height / 2, factor));
        }}
      >
        <option value="" disabled>
          {current}%
        </option>
        {ZOOM_PRESET_PERCENTS.map((p) => (
          <option key={p} value={p}>
            {p}%
          </option>
        ))}
      </select>
    </div>
  );
}

export function FootprintToolbarControl({ control }: { control: string }) {
  const state = useFpState();
  const dispatch = useFpDispatch();
  if (control === "gridSelect") return <GridSelect gridUm={state.gridUm} onChange={(um) => dispatch({ type: "SET_GRID_UM", um })} />;
  if (control === "zoomSelect") return <ZoomSelect view={state.view} onView={(view) => dispatch({ type: "SET_VIEW", view })} />;
  if (control === "layerSelector") {
    return (
      <div className="toolbar-control" title="Layer new graphics and text are drawn on">
        <select value={state.activeLayer} onChange={(e) => dispatch({ type: "SET_ACTIVE_LAYER", layer: e.target.value })}>
          {GRAPHIC_LAYERS.map((l) => (
            <option key={l} value={l}>
              {l}
            </option>
          ))}
        </select>
      </div>
    );
  }
  return null;
}

/** `SYMBOL::SubReference`: unit 1 is "A", 2 is "B", ... (`Unit A` in KiCad's unit box). */
function unitLetter(unit: number): string {
  return unit >= 1 && unit <= 26 ? String.fromCharCode(64 + unit) : String(unit);
}

export function SymbolToolbarControl({ control }: { control: string }) {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const sym = state.symbol;
  if (control === "unitSelector") {
    const units = Array.from({ length: Math.max(1, sym?.unit_count ?? 1) }, (_, i) => i + 1);
    return (
      <div className="toolbar-control" title="Which unit new pins and graphics are placed on">
        <select value={state.activeUnit} onChange={(e) => dispatch({ type: "SET_ACTIVE_UNIT", unit: Number(e.target.value) })} disabled={!sym}>
          {units.map((u) => (
            <option key={u} value={u}>
              {`Unit ${unitLetter(u)}`}
            </option>
          ))}
        </select>
      </div>
    );
  }
  if (control === "bodyStyleSelector") {
    return (
      <div className="toolbar-control" title="Which body style (De Morgan) new pins and graphics are placed on">
        <select value={state.activeBodyStyle} onChange={(e) => dispatch({ type: "SET_ACTIVE_BODY_STYLE", style: Number(e.target.value) })} disabled={!sym?.has_alternate_body_style}>
          <option value={1}>Standard</option>
          <option value={2}>Alternate</option>
        </select>
      </div>
    );
  }
  return null;
}
