// "E" on a selected track/via/zone/shape. Unlike a footprint (rotate/
// flip/move all exist) or text (a full edit_text), these have only
// delete_*/move_* Cmds -- a track additionally has set_track_width, the
// one field of any of these four that's actually editable after the
// fact -- so this is read-only elsewhere, same convention
// FootprintPropertiesDialog already uses for fields this app has no
// command to change.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength, formatXY, umFrom } from "../state/units";

export function ItemPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const id = state.itemPropertiesId;
  const close = () => dispatch({ type: "SET_ITEM_PROPERTIES_ID", id: null });

  const track = id ? api.trackById(id) : undefined;
  const via = id ? api.viaById(id) : undefined;
  const zone = id ? api.zoneById(id) : undefined;
  const shape = id ? api.shapeById(id) : undefined;

  const [width, setWidth] = useState("");

  if (!id || (!track && !via && !zone && !shape)) return null;

  const title = track ? "Track Properties" : via ? "Via Properties" : zone ? "Zone Properties" : "Shape Properties";

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
                  <input placeholder={formatLength(track.width, state.units)} value={width} onChange={(e) => setWidth(e.target.value)} style={{ width: 90 }} />
                </span>
              </>
            )}
            {via && (
              <>
                <span>Net</span>
                <span>{via.net}</span>
                <span>Position</span>
                <span>{formatXY(via.x, via.y, state.units)}</span>
                <span>Diameter</span>
                <span>{formatLength(via.d, state.units)}</span>
                <span>Drill</span>
                <span>{formatLength(via.drill, state.units)}</span>
                <span>Layers</span>
                <span>
                  {via.from} - {via.to}
                </span>
              </>
            )}
            {zone && (
              <>
                <span>Net</span>
                <span>{zone.net}</span>
                <span>Layer</span>
                <span>{zone.layer}</span>
                <span>Points</span>
                <span>{zone.outline.length}</span>
              </>
            )}
            {shape && (
              <>
                <span>Kind</span>
                <span>{shape.kind}</span>
                <span>Layer</span>
                <span>{shape.layer}</span>
                <span>Line width</span>
                <span>{formatLength(shape.stroke_width, state.units)}</span>
              </>
            )}
          </div>
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            {track ? "Width is the only field this app can change after the fact -- everything else has no edit command yet, only delete." : "Read-only: no edit command exists for this item yet, only move and delete."}
          </p>
        </div>
        <div className="dialog-footer">
          {track && width.trim() && (
            <button
              onClick={() => {
                const um = Math.round(umFrom(Number(width), state.units));
                if (Number.isFinite(um) && um > 0) api.cmd({ op: "set_track_width", id, width: um });
                setWidth("");
              }}
            >
              Apply Width
            </button>
          )}
          <button
            onClick={() => {
              if (track) api.cmd({ op: "delete_track", id });
              else if (via) api.cmd({ op: "delete_via", id });
              else if (zone) api.cmd({ op: "delete_zone", id });
              else if (shape) api.cmd({ op: "delete_shape", id });
              close();
            }}
          >
            Delete
          </button>
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
