// Port of pcbnew/dialogs/dialog_create_array{,_base}.cpp -- "Create
// Array..." (pcbnew.Array.createArray, Ctrl+T, task item 6). Scoped to
// the board editor half of source's dialog: `ARRAY_TOOL::CreateArray`'s
// own `enableArrayNumbering = m_isFootprintEditor` means the footprint-
// editor pad-numbering panel (`ARRAY_PAD_NUMBER_PROVIDER`, the
// `ARRAY_AXIS` numeric/hex/alphabetic schemes) never shows in the board
// editor either -- this app's footprint editor has no multi-pad array
// tooling of its own to hang that half on yet, see PARITY-pcb.md section
// 17. Footprint reannotation (`m_radioBtnKeepRefs`/`m_radioBtnUniqueRefs`)
// is not ported either -- a placed part's id *is* its schematic symbol's
// id, so there is no "assign a fresh unique reference" this app's ops
// layer could do without breaking that link.
//
// "Duplicate" (default) vs "Arrange selection" mirrors source's own
// `m_radioBtnDuplicateSelection`/`m_radioBtnArrangeSelection`. Angles are
// plain typed magnitudes plus a Clockwise/Counterclockwise direction
// radio, same as source's `m_rbCircDirection` -- unlike MoveExactDialog's
// single signed rotation field, this needs no CCW-positive-then-negate
// bridge (see `ArrayGeometry`'s own doc in api/types.ts).
import { useMemo, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, umTo, type LengthUnit } from "../state/units";
import type { ArrayGeometry } from "../api/types";

type Tab = "grid" | "circular";

/** This item's own reference point, in whichever of this model's five
 * arrayable kinds (plus a placed part, arrange-mode only) it names -- the
 * same per-kind anchor `array_offset`/`ArrayGeometry::transform` use on
 * the backend. `null` for a group or an unknown id (skipped there too). */
function referencePointUm(api: ReturnType<typeof useStudioApi>, id: string): [number, number] | null {
  const part = api.partByRef(id);
  if (part?.placed && part.at) return part.at;
  const via = api.viaById(id);
  if (via) return [via.x, via.y];
  const text = api.textById(id);
  if (text) return [text.x, text.y];
  const track = api.trackById(id);
  if (track?.pts[0]) return track.pts[0];
  const zone = api.zoneById(id);
  if (zone?.outline[0]) return zone.outline[0];
  const shape = api.shapeById(id);
  if (shape) {
    const pt = "start" in shape ? shape.start : "center" in shape ? shape.center : shape.pts[0];
    if (pt) return pt;
  }
  return null;
}

export function CreateArrayDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.createArrayDialogOpen;
  const unit: LengthUnit = state.units;

  const [tab, setTab] = useState<Tab>("grid");
  const [arrange, setArrange] = useState(false);

  // Grid, source's own CREATE_ARRAY_DIALOG_ENTRIES defaults (5x5, 2.54mm pitch).
  const [nx, setNx] = useState(5);
  const [ny, setNy] = useState(5);
  const [dx, setDx] = useState(2.54);
  const [dy, setDy] = useState(2.54);
  const [offsetX, setOffsetX] = useState(0);
  const [offsetY, setOffsetY] = useState(0);
  const [stagger, setStagger] = useState(0);
  const [staggerRows, setStaggerRows] = useState(true);
  const [centred, setCentred] = useState(false);

  // Circular, source's own defaults (4 points, 90 degrees apart, clockwise).
  const [centerX, setCenterX] = useState(0);
  const [centerY, setCenterY] = useState(0);
  const [centerInit, setCenterInit] = useState(false);
  const [count, setCount] = useState(4);
  const [divideEvenly, setDivideEvenly] = useState(false);
  const [angleDeg, setAngleDeg] = useState(90);
  const [angleOffsetDeg, setAngleOffsetDeg] = useState(0);
  const [clockwise, setClockwise] = useState(true);
  const [rotateItems, setRotateItems] = useState(false);

  const [busy, setBusy] = useState(false);

  const ids = useMemo(() => [...state.selection], [state.selection]);

  // One-time default for the circular center: the average reference
  // point of whatever is selected -- `ARRAY_TOOL::CreateArray`'s own
  // `origin` (a single item's own position, or the selection's center).
  if (open && !centerInit && ids.length > 0) {
    const pts = ids.map((id) => referencePointUm(api, id)).filter((p): p is [number, number] => p != null);
    if (pts.length > 0) {
      const avgX = pts.reduce((s, p) => s + p[0], 0) / pts.length;
      const avgY = pts.reduce((s, p) => s + p[1], 0) / pts.length;
      setCenterX(Math.round(umTo(avgX, unit) * 1000) / 1000);
      setCenterY(Math.round(umTo(avgY, unit) * 1000) / 1000);
    }
    setCenterInit(true);
  }

  if (!open) return null;

  const close = () => {
    dispatch({ type: "SET_CREATE_ARRAY_DIALOG_OPEN", open: false });
    // Reset to source's own defaults for next time, same as every other
    // one-shot dialog in this app (CleanupTracksDialog.tsx etc).
    setTab("grid");
    setArrange(false);
    setCentred(false);
    setStagger(0);
    setCenterInit(false);
    setDivideEvenly(false);
  };

  const effectiveAngleDeg = divideEvenly && count > 0 ? 360 / count : angleDeg;

  const geometry: ArrayGeometry =
    tab === "grid"
      ? {
          kind: "grid",
          nx,
          ny,
          dx: Math.round(umFrom(dx, unit)),
          dy: Math.round(umFrom(dy, unit)),
          offset_x: Math.round(umFrom(offsetX, unit)),
          offset_y: Math.round(umFrom(offsetY, unit)),
          centred,
          stagger,
          stagger_rows: staggerRows,
          horizontal_then_vertical: true,
        }
      : {
          kind: "circular",
          center: { x: Math.round(umFrom(centerX, unit)), y: Math.round(umFrom(centerY, unit)) },
          count,
          angle_millideg: Math.round(effectiveAngleDeg * 1000),
          angle_offset_millideg: Math.round(angleOffsetDeg * 1000),
          clockwise,
          rotate_items: rotateItems,
        };

  const valid = tab === "grid" ? nx >= 1 && ny >= 1 && (nx === 1 || dx !== 0) && (ny === 1 || dy !== 0) : count >= 1 && (count === 1 || effectiveAngleDeg !== 0);

  const create = async () => {
    if (ids.length === 0 || !valid) return;
    setBusy(true);
    try {
      const ok = await api.cmd({ op: "create_array", ids, geometry, arrange });
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Create Array</span>
        </div>
        <div className="dialog-body">
          <div style={{ display: "flex", gap: 4, marginBottom: 10 }}>
            <button className={tab === "grid" ? "primary" : undefined} onClick={() => setTab("grid")} style={{ flex: 1 }}>
              Grid
            </button>
            <button className={tab === "circular" ? "primary" : undefined} onClick={() => setTab("circular")} style={{ flex: 1 }}>
              Circular
            </button>
          </div>

          {tab === "grid" && (
            <div>
              <div style={{ display: "flex", gap: 8 }}>
                <label style={{ flex: 1 }}>
                  Horizontal count
                  <input type="number" min={1} step={1} value={nx} onChange={(e) => setNx(Math.max(1, Math.round(Number(e.target.value))))} style={{ width: "100%" }} />
                </label>
                <label style={{ flex: 1 }}>
                  Vertical count
                  <input type="number" min={1} step={1} value={ny} onChange={(e) => setNy(Math.max(1, Math.round(Number(e.target.value))))} style={{ width: "100%" }} />
                </label>
              </div>
              <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
                <label style={{ flex: 1 }}>
                  Spacing X ({unit})
                  <input type="number" step="any" value={dx} onChange={(e) => setDx(Number(e.target.value))} style={{ width: "100%" }} />
                </label>
                <label style={{ flex: 1 }}>
                  Spacing Y ({unit})
                  <input type="number" step="any" value={dy} onChange={(e) => setDy(Number(e.target.value))} style={{ width: "100%" }} />
                </label>
              </div>
              <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
                <label style={{ flex: 1 }}>
                  Offset X ({unit})
                  <input type="number" step="any" value={offsetX} onChange={(e) => setOffsetX(Number(e.target.value))} style={{ width: "100%" }} />
                </label>
                <label style={{ flex: 1 }}>
                  Offset Y ({unit})
                  <input type="number" step="any" value={offsetY} onChange={(e) => setOffsetY(Number(e.target.value))} style={{ width: "100%" }} />
                </label>
              </div>
              <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 6 }}>
                <label style={{ flex: 1 }} title="A brick/honeycomb offset every Nth row or column; 0 or 1 disables it.">
                  Stagger
                  <input type="number" step={1} value={stagger} onChange={(e) => setStagger(Math.round(Number(e.target.value)))} style={{ width: "100%" }} />
                </label>
                <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 14 }}>
                  <input type="radio" checked={staggerRows} onChange={() => setStaggerRows(true)} /> Rows
                </label>
                <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 14 }}>
                  <input type="radio" checked={!staggerRows} onChange={() => setStaggerRows(false)} /> Columns
                </label>
              </div>
              <div style={{ marginTop: 8 }}>
                <label className="filter-row" style={{ display: "block" }}>
                  <input type="radio" checked={!centred} onChange={() => setCentred(false)} /> Keep the original position at one corner
                </label>
                <label className="filter-row" style={{ display: "block" }}>
                  <input type="radio" checked={centred} onChange={() => setCentred(true)} /> Centre the array on the original position
                </label>
              </div>
            </div>
          )}

          {tab === "circular" && (
            <div>
              <div style={{ display: "flex", gap: 8 }}>
                <label style={{ flex: 1 }}>
                  Centre X ({unit})
                  <input type="number" step="any" value={centerX} onChange={(e) => setCenterX(Number(e.target.value))} style={{ width: "100%" }} />
                </label>
                <label style={{ flex: 1 }}>
                  Centre Y ({unit})
                  <input type="number" step="any" value={centerY} onChange={(e) => setCenterY(Number(e.target.value))} style={{ width: "100%" }} />
                </label>
              </div>
              <label style={{ display: "block", marginTop: 6 }}>
                Point count
                <input type="number" min={1} step={1} value={count} onChange={(e) => setCount(Math.max(1, Math.round(Number(e.target.value))))} style={{ width: "100%" }} />
              </label>
              <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 6 }}>
                <input type="checkbox" checked={divideEvenly} onChange={(e) => setDivideEvenly(e.target.checked)} /> Divide evenly (360&deg; / count)
              </label>
              <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
                <label style={{ flex: 1 }}>
                  Angle between points (&deg;)
                  <input type="number" step="any" value={divideEvenly ? Math.round(effectiveAngleDeg * 1000) / 1000 : angleDeg} onChange={(e) => setAngleDeg(Number(e.target.value))} disabled={divideEvenly} style={{ width: "100%" }} />
                </label>
                <label style={{ flex: 1 }}>
                  Angle offset (&deg;)
                  <input type="number" step="any" value={angleOffsetDeg} onChange={(e) => setAngleOffsetDeg(Number(e.target.value))} style={{ width: "100%" }} />
                </label>
              </div>
              <div style={{ marginTop: 8 }}>
                <label className="filter-row" style={{ display: "block" }}>
                  <input type="radio" checked={clockwise} onChange={() => setClockwise(true)} /> Clockwise
                </label>
                <label className="filter-row" style={{ display: "block" }}>
                  <input type="radio" checked={!clockwise} onChange={() => setClockwise(false)} /> Counterclockwise
                </label>
              </div>
              <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 6 }} title="Also spin each item in place by the same angle -- only a footprint or text actually turns; a track/via/zone/shape only ever moves along the circle (see PARITY-pcb.md section 17).">
                <input type="checkbox" checked={rotateItems} onChange={(e) => setRotateItems(e.target.checked)} /> Rotate items as they are placed
              </label>
            </div>
          )}

          <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 12, paddingTop: 8, borderTop: "1px solid var(--chrome-border, #333)" }}>
            <input type="checkbox" checked={arrange} onChange={(e) => setArrange(e.target.checked)} /> Arrange selection (reposition the {ids.length} selected item{ids.length === 1 ? "" : "s"} instead of duplicating)
          </label>
          {ids.length === 0 && <div style={{ fontSize: 11, opacity: 0.7, marginTop: 4 }}>Select at least one item first.</div>}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={create} disabled={busy || ids.length === 0 || !valid}>
            Create Array
          </button>
        </div>
      </div>
    </div>
  );
}

