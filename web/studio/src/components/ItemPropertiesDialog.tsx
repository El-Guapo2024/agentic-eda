// "E" on a selected track/via/shape (a zone gets its own full dialog --
// ZoneDialog.tsx, same one "Add Zone" uses). dialog_track_via_properties.cpp
// (track width; via diameter/drill) and pcb_shape's own properties
// (layer/line width/filled) are now editable through `set_track_width`/
// `edit_via`/`edit_shape` (GAPS.md #11) -- net, position and a via's
// layer span stay read-only, same as real KiCad's own dialogs (re-netting
// or re-spanning an existing via isn't a field edit there either, it's
// delete-and-redraw).
import { useEffect, useState } from "react";
import { STANDARD_LAYERS } from "./canvas/layers";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength, formatXY, umFrom, umTo } from "../state/units";

export function ItemPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const id = state.itemPropertiesId;
  const close = () => dispatch({ type: "SET_ITEM_PROPERTIES_ID", id: null });

  const track = id ? api.trackById(id) : undefined;
  const via = id ? api.viaById(id) : undefined;
  const shape = id ? api.shapeById(id) : undefined;
  const units = state.units;

  const [width, setWidth] = useState("");
  const [viaDiameter, setViaDiameter] = useState(0);
  const [viaDrill, setViaDrill] = useState(0);
  const [shapeLayer, setShapeLayer] = useState("");
  const [shapeWidth, setShapeWidth] = useState(0);
  const [shapeFilled, setShapeFilled] = useState(false);

  // Populate the editable copies whenever the dialog opens on a
  // (possibly different) item -- not every render, or a mid-edit
  // keystroke would be clobbered by the next /api/state poll.
  useEffect(() => {
    if (via) {
      setViaDiameter(umTo(via.d, units));
      setViaDrill(umTo(via.drill, units));
    }
    if (shape) {
      setShapeLayer(shape.layer);
      setShapeWidth(umTo(shape.stroke_width, units));
      setShapeFilled(shape.filled);
    }
    setWidth("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || (!track && !via && !shape)) return null;

  const title = track ? "Track Properties" : via ? "Via Properties" : "Shape Properties";
  const layerOptions = [...(state.board?.layers ?? []), ...STANDARD_LAYERS.map((l) => l.label)];

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid">
            {track && (
              <>
                <span>Net</span>
                <span>{track.net}</span>
                <span>Layer</span>
                <span>{track.layer}</span>
                <span>Width</span>
                <span>
                  <input placeholder={formatLength(track.width, units)} value={width} onChange={(e) => setWidth(e.target.value)} style={{ width: 90 }} />
                </span>
              </>
            )}
            {via && (
              <>
                <span>Net</span>
                <span>{via.net}</span>
                <span>Position</span>
                <span>{formatXY(via.x, via.y, units)}</span>
                <span>Diameter</span>
                <span>
                  <input type="number" step="any" value={viaDiameter} onChange={(e) => setViaDiameter(Number(e.target.value) || 0)} style={{ width: 90 }} /> {units}
                </span>
                <span>Drill</span>
                <span>
                  <input type="number" step="any" value={viaDrill} onChange={(e) => setViaDrill(Number(e.target.value) || 0)} style={{ width: 90 }} /> {units}
                </span>
                <span>Layers</span>
                <span>
                  {via.from} - {via.to}
                </span>
              </>
            )}
            {shape && (
              <>
                <span>Kind</span>
                <span>{shape.kind}</span>
                <span>Layer</span>
                <span>
                  <select value={shapeLayer} onChange={(e) => setShapeLayer(e.target.value)}>
                    {layerOptions.map((l) => (
                      <option key={l} value={l}>
                        {l}
                      </option>
                    ))}
                  </select>
                </span>
                <span>Line width</span>
                <span>
                  <input type="number" step="any" value={shapeWidth} onChange={(e) => setShapeWidth(Number(e.target.value) || 0)} style={{ width: 90 }} /> {units}
                </span>
                <span>Filled</span>
                <span>
                  <input type="checkbox" checked={shapeFilled} onChange={(e) => setShapeFilled(e.target.checked)} />
                </span>
              </>
            )}
          </div>
          {track && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>Net and layer have no edit command yet -- delete and redraw to change either.</p>}
          {via && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>Net and layer span have no edit command yet, same as real KiCad's own dialog -- delete and redraw to change either.</p>}
        </div>
        <div className="dialog-footer">
          {track && (
            <button
              disabled={!width.trim()}
              onClick={() => {
                const um = Math.round(umFrom(Number(width), units));
                if (Number.isFinite(um) && um > 0) api.cmd({ op: "set_track_width", id, width: um });
                setWidth("");
              }}
            >
              Apply Width
            </button>
          )}
          {via && (
            <button
              className="primary"
              onClick={async () => {
                const diameter = Math.round(umFrom(viaDiameter, units));
                const drill = Math.round(umFrom(viaDrill, units));
                if (await api.cmd({ op: "edit_via", id, diameter, drill })) close();
              }}
            >
              Apply
            </button>
          )}
          {shape && (
            <button
              className="primary"
              onClick={async () => {
                const stroke_width = Math.round(umFrom(shapeWidth, units));
                if (await api.cmd({ op: "edit_shape", id, layer: shapeLayer, stroke_width, filled: shapeFilled })) close();
              }}
            >
              Apply
            </button>
          )}
          <button
            onClick={() => {
              if (track) api.cmd({ op: "delete_track", id });
              else if (via) api.cmd({ op: "delete_via", id });
              else if (shape) api.cmd({ op: "delete_shape", id });
              close();
            }}
          >
            Delete
          </button>
          <button onClick={close}>{via || shape ? "Cancel" : "Close"}</button>
        </div>
      </div>
    </div>
  );
}
