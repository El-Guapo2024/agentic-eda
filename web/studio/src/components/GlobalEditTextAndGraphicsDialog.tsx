// Port of pcbnew/dialogs/dialog_global_edit_text_and_graphics{,_base}.cpp
// -- "Edit Text & Graphics Properties..."
// (pcbnew.GlobalEdit.editTextAndGraphics, task item 2), scoped to the
// free-standing board items this model has: graphic items (`Shape`), text
// items (`Text`) and dimensions. The Scope box and the Filter Items box (by
// layer, selected items only) are kicad-port/globalEditTextGraphics.ts.
// Source's footprint reference / value / other-field / text / graphic /
// dimension scopes, the "By parent reference designator" and "By parent
// footprint library link" filters, and the font, bold, italic, keep-upright,
// visible and "center on footprint" rows have nothing to act on here -- a
// footprint's reference and value live on the read-only intent-derived
// `Part` (PARITY-pcb.md section 10), and a board text has one square size,
// no font and no style flags.
//
// "Set to layer default values:" (`StyleFromSettings`) gives each item the
// line thickness, text size and text thickness Board Setup > Text &
// Graphics > Defaults holds for its layer class (crates/ops/src/pcb_global_edit.rs),
// and a dimension its dimension settings; "Set to specified values:" sets
// what is ticked.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, type LengthUnit } from "../state/units";
import { initialTextGraphicsFilter, matchingTextAndGraphics, NOTHING_IN_TEXT_SCOPE, textGraphicsLayers, type TextGraphicsFilter, type TextGraphicsScope } from "../kicad-port/globalEditTextGraphics";

type Mode = "specified" | "layer_defaults";

const GROUP_TITLE = { fontWeight: 600, fontSize: 11, marginBottom: 4 } as const;

export function GlobalEditTextAndGraphicsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.editTextAndGraphicsDialogOpen;

  const [scope, setScope] = useState<TextGraphicsScope>(NOTHING_IN_TEXT_SCOPE);
  // null until a filter is touched: the layer follows the active layer of the moment the dialog opens.
  const [box, setBox] = useState<TextGraphicsFilter | null>(null);

  const [mode, setMode] = useState<Mode>("specified");
  const [changeLayer, setChangeLayer] = useState(false);
  const [newLayer, setNewLayer] = useState("");
  const [changeLineWidth, setChangeLineWidth] = useState(false);
  const [lineWidth, setLineWidth] = useState(0); // display units, graphic items and dimensions
  const [changeTextSize, setChangeTextSize] = useState(false);
  const [textSize, setTextSize] = useState(0); // display units, texts and dimensions
  const [changeTextThickness, setChangeTextThickness] = useState(false);
  const [textThickness, setTextThickness] = useState(0); // display units, texts and dimensions
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const layers = textGraphicsLayers(state.board);
  const filter = box ?? initialTextGraphicsFilter(state.board, state.activeLayer);
  const setFilter = (patch: Partial<TextGraphicsFilter>) => setBox({ ...filter, ...patch });

  const close = () => {
    setScope(NOTHING_IN_TEXT_SCOPE);
    setBox(null);
    dispatch({ type: "SET_EDIT_TEXT_AND_GRAPHICS_DIALOG_OPEN", open: false });
  };

  const { shapeIds, textIds, dimensionIds } = matchingTextAndGraphics(state.board, scope, filter, state.selection);
  const unit: LengthUnit = state.units;
  const total = shapeIds.length + textIds.length + dimensionIds.length;
  const anyChange = mode === "layer_defaults" || changeLayer || changeLineWidth || changeTextSize || changeTextThickness;

  const apply = async () => {
    if (total === 0 || !anyChange) return;
    setBusy(true);
    try {
      const ok = await api.cmd(
        mode === "layer_defaults"
          ? { op: "edit_text_and_graphics", shape_ids: shapeIds, text_ids: textIds, dimension_ids: dimensionIds, layer_defaults: true }
          : {
              op: "edit_text_and_graphics",
              shape_ids: shapeIds,
              text_ids: textIds,
              dimension_ids: dimensionIds,
              layer: changeLayer && newLayer ? newLayer : null,
              line_width: changeLineWidth ? Math.round(umFrom(lineWidth, unit)) : null,
              text_size: changeTextSize ? Math.round(umFrom(textSize, unit)) : null,
              text_thickness: changeTextThickness ? Math.round(umFrom(textThickness, unit)) : null,
            },
      );
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  const specified = mode === "specified";

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Edit Text & Graphics Properties</span>
        </div>
        <div className="dialog-body">
          <div style={GROUP_TITLE}>Scope</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="checkbox" checked={scope.graphics} onChange={(e) => setScope({ ...scope, graphics: e.target.checked })} aria-label="PCB graphic items" /> PCB graphic items
          </label>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="checkbox" checked={scope.text} onChange={(e) => setScope({ ...scope, text: e.target.checked })} aria-label="PCB text items" /> PCB text items
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 8 }}>
            <input type="checkbox" checked={scope.dimensions} onChange={(e) => setScope({ ...scope, dimensions: e.target.checked })} aria-label="PCB dimensions" /> PCB dimensions
          </label>

          <div style={GROUP_TITLE}>Filter Items</div>
          <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 4 }}>
            <input type="checkbox" checked={filter.layerOn} onChange={(e) => setFilter({ layerOn: e.target.checked })} aria-label="Filter by layer" /> By layer:
            <select value={filter.layer} onChange={(e) => setFilter({ layer: e.target.value, layerOn: true })} style={{ flex: 1 }} aria-label="Layer filter">
              {layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 8 }}>
            <input type="checkbox" checked={filter.selectedOnly} onChange={(e) => setFilter({ selectedOnly: e.target.checked })} aria-label="Selected items only" /> Selected items only
          </label>

          <div style={GROUP_TITLE}>Action</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="radio" checked={specified} onChange={() => setMode("specified")} aria-label="Set to specified values" /> Set to specified values:
          </label>
          <div className="kv-grid" style={{ gridTemplateColumns: "20px 130px 1fr", rowGap: 4, marginLeft: 20, marginBottom: 6, opacity: specified ? 1 : 0.5 }}>
            <input type="checkbox" checked={changeLayer} disabled={!specified} onChange={(e) => setChangeLayer(e.target.checked)} />
            <span>Layer:</span>
            <select value={newLayer} onChange={(e) => setNewLayer(e.target.value)} disabled={!specified || !changeLayer}>
              <option value="">-- leave unchanged --</option>
              {layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>

            <input type="checkbox" checked={changeLineWidth} disabled={!specified} onChange={(e) => setChangeLineWidth(e.target.checked)} />
            <span>Line thickness:</span>
            <input type="number" value={lineWidth} disabled={!specified || !changeLineWidth} onChange={(e) => setLineWidth(Number(e.target.value))} />

            <input type="checkbox" checked={changeTextSize} disabled={!specified} onChange={(e) => setChangeTextSize(e.target.checked)} />
            <span>Text size:</span>
            <input type="number" value={textSize} disabled={!specified || !changeTextSize} onChange={(e) => setTextSize(Number(e.target.value))} />

            <input type="checkbox" checked={changeTextThickness} disabled={!specified} onChange={(e) => setChangeTextThickness(e.target.checked)} />
            <span>Text thickness:</span>
            <input type="number" value={textThickness} disabled={!specified || !changeTextThickness} onChange={(e) => setTextThickness(Number(e.target.value))} />
          </div>
          <label className="filter-row" style={{ display: "block" }} title="Each item gets the line thickness and text size its layer class has in Board Setup > Text & Graphics > Defaults; a dimension also gets the dimension settings">
            <input type="radio" checked={!specified} onChange={() => setMode("layer_defaults")} aria-label="Set to layer default values" /> {scope.dimensions ? "Set to layer and dimension default values:" : "Set to layer default values:"}
          </label>

          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 10 }}>
            {shapeIds.length} graphic item(s), {textIds.length} text(s) and {dimensionIds.length} dimension(s) match the current scope and filters.
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
          <button className="primary" onClick={apply} disabled={busy || total === 0 || !anyChange}>
            Apply and Close
          </button>
        </div>
      </div>
    </div>
  );
}
