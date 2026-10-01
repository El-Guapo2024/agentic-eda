// Port of pcbnew/dialogs/dialog_global_edit_text_and_graphics{,_base}.cpp
// -- "Edit Text & Graphics Properties..."
// (pcbnew.GlobalEdit.editTextAndGraphics, task item 2), scoped to this
// model's two free-standing board drawing kinds: `Shape` (silkscreen/
// fab/edge art) and `Text`. Source's own footprint reference/value/other-
// field checkboxes, dimension items, tables and barcodes have no
// counterpart here -- a footprint's reference/value live on the read-only
// intent-derived `Part` (PARITY-pcb.md section 10), and this model has no
// dimension/table/barcode item at all (dimensions are GAPS.md #28,
// tracked separately). There is also no "layer default values" mode --
// this model has no `BOARD_DESIGN_SETTINGS::m_LineThickness`/`m_TextSize`
// per-layer-class arrays to reset to -- so unlike source there is only
// the one "specified values" action.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, type LengthUnit } from "../state/units";

export function GlobalEditTextAndGraphicsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.editTextAndGraphicsDialogOpen;

  const [scopeGraphics, setScopeGraphics] = useState(false);
  const [scopeText, setScopeText] = useState(false);
  const [layerFilterOn, setLayerFilterOn] = useState(false);
  const [layerFilter, setLayerFilter] = useState("");
  const [selectedOnly, setSelectedOnly] = useState(false);

  const [changeLayer, setChangeLayer] = useState(false);
  const [newLayer, setNewLayer] = useState("");
  const [changeLineWidth, setChangeLineWidth] = useState(false);
  const [lineWidth, setLineWidth] = useState(0); // display units, shapes only
  const [changeTextSize, setChangeTextSize] = useState(false);
  const [textSize, setTextSize] = useState(0); // display units, texts only
  const [changeTextThickness, setChangeTextThickness] = useState(false);
  const [textThickness, setTextThickness] = useState(0); // display units, texts only
  const [busy, setBusy] = useState(false);

  const layers = state.board?.layers ?? [];
  // Graphics/text live on any layer (silkscreen, fab, edge-cuts, ...), not
  // only copper -- the full board layer *set* this app knows about has no
  // single home elsewhere, so build it from what's actually drawn plus
  // the copper list, same spirit `ZoneDialog.tsx`'s net picker builds its
  // options from what's actually on the board rather than a fixed catalog.
  const allLayers = (() => {
    const set = new Set(layers);
    for (const s of state.board?.drawings?.shapes ?? []) set.add(s.layer);
    for (const t of state.board?.drawings?.texts ?? []) set.add(t.layer);
    return [...set].sort();
  })();

  if (!open) return null;

  const close = () => dispatch({ type: "SET_EDIT_TEXT_AND_GRAPHICS_DIALOG_OPEN", open: false });

  const passFilters = (layer: string, id: string) => {
    if (layerFilterOn && layerFilter && layer !== layerFilter) return false;
    if (selectedOnly && !state.selection.has(id)) return false;
    return true;
  };

  const shapeIds = scopeGraphics ? (state.board?.drawings?.shapes ?? []).filter((s) => passFilters(s.layer, s.id)).map((s) => s.id) : [];
  const textIds = scopeText ? (state.board?.drawings?.texts ?? []).filter((t) => passFilters(t.layer, t.id)).map((t) => t.id) : [];
  const unit: LengthUnit = state.units;
  const total = shapeIds.length + textIds.length;
  const anyChange = changeLayer || changeLineWidth || changeTextSize || changeTextThickness;

  const apply = async () => {
    if (total === 0 || !anyChange) return;
    setBusy(true);
    try {
      const ok = await api.cmd({
        op: "edit_text_and_graphics",
        shape_ids: shapeIds,
        text_ids: textIds,
        layer: changeLayer && newLayer ? newLayer : null,
        line_width: changeLineWidth ? Math.round(umFrom(lineWidth, unit)) : null,
        text_size: changeTextSize ? Math.round(umFrom(textSize, unit)) : null,
        text_thickness: changeTextThickness ? Math.round(umFrom(textThickness, unit)) : null,
      });
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Edit Text & Graphics Properties</span>
        </div>
        <div className="dialog-body">
          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Scope</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="checkbox" checked={scopeGraphics} onChange={(e) => setScopeGraphics(e.target.checked)} /> Board graphics (shapes)
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 8 }}>
            <input type="checkbox" checked={scopeText} onChange={(e) => setScopeText(e.target.checked)} /> Board text
          </label>

          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Filter Items</div>
          <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <input type="checkbox" checked={layerFilterOn} onChange={(e) => setLayerFilterOn(e.target.checked)} /> Layer:
            <select value={layerFilter} onChange={(e) => setLayerFilter(e.target.value)} disabled={!layerFilterOn} style={{ flex: 1 }}>
              <option value="">(any)</option>
              {allLayers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={{ display: "block", marginTop: 4, marginBottom: 8 }}>
            <input type="checkbox" checked={selectedOnly} onChange={(e) => setSelectedOnly(e.target.checked)} /> Selected items only
          </label>

          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Set to specified values</div>
          <div className="kv-grid" style={{ gridTemplateColumns: "20px 110px 1fr", rowGap: 4 }}>
            <input type="checkbox" checked={changeLayer} onChange={(e) => setChangeLayer(e.target.checked)} />
            <span>Layer:</span>
            <select value={newLayer} onChange={(e) => setNewLayer(e.target.value)} disabled={!changeLayer}>
              <option value="">-- leave unchanged --</option>
              {allLayers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>

            <input type="checkbox" checked={changeLineWidth} onChange={(e) => setChangeLineWidth(e.target.checked)} />
            <span>Line width (shapes):</span>
            <input type="number" value={lineWidth} disabled={!changeLineWidth} onChange={(e) => setLineWidth(Number(e.target.value))} />

            <input type="checkbox" checked={changeTextSize} onChange={(e) => setChangeTextSize(e.target.checked)} />
            <span>Text size (texts):</span>
            <input type="number" value={textSize} disabled={!changeTextSize} onChange={(e) => setTextSize(Number(e.target.value))} />

            <input type="checkbox" checked={changeTextThickness} onChange={(e) => setChangeTextThickness(e.target.checked)} />
            <span>Text thickness (texts):</span>
            <input type="number" value={textThickness} disabled={!changeTextThickness} onChange={(e) => setTextThickness(Number(e.target.value))} />
          </div>

          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 10 }}>{total} item(s) match the current scope and filters.</div>
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
