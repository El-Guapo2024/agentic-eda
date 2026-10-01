// Port of eeschema/dialogs/dialog_lib_edit_pin_table.cpp -- one row per
// pin (source's own row can merge several identical pins into one; this
// app's simpler one-physical-pin-per-row model skips that grouping, a
// documented simplification, see PARITY-symedit.md), every field
// inline-editable, sorted by (unit, number) same as source's own default
// secondary sort key. Not ported: "Group by name", "Group Selected
// Pins", column-header sort, and CSV/clipboard import-export -- this
// app's own symbols are small enough in practice that a plain, always-
// sorted table covers the real need; see that same doc for the list.
import { useState } from "react";
import type { LibrarySymbolPin, PinElectricalType, PinShape } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { angleDegToOrientation, orientationToAngleDeg, PIN_ORIENTATIONS, type PinOrientation } from "../../kicad-port/pinOrientation";
import { nextPinNumber } from "../../kicad-port/pinNumbering";

const ELECTRICAL_TYPES: PinElectricalType[] = ["input", "output", "bidirectional", "tri_state", "passive", "free", "unspecified", "power_in", "power_out", "open_collector", "open_emitter", "no_connect"];
const SHAPES: PinShape[] = ["line", "inverted", "clock", "inverted_clock", "input_low", "clock_low", "output_low", "edge_clock_high", "non_logic"];
const ORIENTATION_LABELS: Record<PinOrientation, string> = { right: "Right", left: "Left", up: "Up", down: "Down" };

function Row({ pin }: { pin: LibrarySymbolPin }) {
  const api = useSymApi();
  const [local, setLocal] = useState(pin);
  // The poll (~700ms) can hand back a fresher copy than this row's own
  // in-progress edit; only resync when nothing here is actively being
  // typed (no pending text-field edit to clobber) -- approximated by
  // just trusting the id match and always taking the server's value
  // for fields this row isn't the one that just changed. Simpler: stage
  // local edits, commit each field's own change immediately (selects/
  // checkboxes) or on blur (text/number fields), same as the rest of
  // this component's own "server is the source of truth" convention.
  const commit = (next: LibrarySymbolPin) => {
    setLocal(next);
    void api.editPin(pin.id!, next);
  };

  return (
    <tr>
      <td>
        <input value={local.number} onChange={(e) => setLocal({ ...local, number: e.target.value })} onBlur={() => commit(local)} style={{ width: 50 }} />
      </td>
      <td>
        <input value={local.name} onChange={(e) => setLocal({ ...local, name: e.target.value })} onBlur={() => commit(local)} style={{ width: 70 }} />
      </td>
      <td>
        <select value={local.electrical_type} onChange={(e) => commit({ ...local, electrical_type: e.target.value as PinElectricalType })}>
          {ELECTRICAL_TYPES.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
      </td>
      <td>
        <select value={local.shape} onChange={(e) => commit({ ...local, shape: e.target.value as PinShape })}>
          {SHAPES.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
      </td>
      <td>
        <select value={angleDegToOrientation(local.angle_deg)} onChange={(e) => commit({ ...local, angle_deg: orientationToAngleDeg(e.target.value as PinOrientation) })}>
          {PIN_ORIENTATIONS.map((o) => (
            <option key={o} value={o}>
              {ORIENTATION_LABELS[o]}
            </option>
          ))}
        </select>
      </td>
      <td>
        <input type="number" step="any" value={local.at.x} onChange={(e) => setLocal({ ...local, at: { ...local.at, x: Number(e.target.value) } })} onBlur={() => commit(local)} style={{ width: 60 }} />
      </td>
      <td>
        <input type="number" step="any" value={local.at.y} onChange={(e) => setLocal({ ...local, at: { ...local.at, y: Number(e.target.value) } })} onBlur={() => commit(local)} style={{ width: 60 }} />
      </td>
      <td>
        <input type="number" step="any" min={0} value={local.length_mm} onChange={(e) => setLocal({ ...local, length_mm: Number(e.target.value) })} onBlur={() => commit(local)} style={{ width: 55 }} />
      </td>
      <td>
        <input type="number" step="any" min={0} value={local.unit} onChange={(e) => commit({ ...local, unit: Math.max(0, Math.round(Number(e.target.value) || 0)) })} style={{ width: 40 }} />
      </td>
      <td>
        <input type="number" step="any" min={0} max={2} value={local.body_style} onChange={(e) => commit({ ...local, body_style: Math.max(0, Math.round(Number(e.target.value) || 0)) })} style={{ width: 40 }} />
      </td>
      <td>
        <input type="checkbox" checked={!local.hidden} onChange={(e) => commit({ ...local, hidden: !e.target.checked })} />
      </td>
      <td>
        <button onClick={() => void api.deletePin(pin.id!)}>Delete</button>
      </td>
    </tr>
  );
}

export function PinTableDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  if (!state.pinTableOpen) return null;
  const close = () => dispatch({ type: "SET_PIN_TABLE_OPEN", open: false });
  const pins = [...(state.symbol?.pins ?? [])].sort((a, b) => a.unit - b.unit || a.number.localeCompare(b.number, undefined, { numeric: true }));

  const addPin = () => {
    const number = nextPinNumber(state.symbol?.pins ?? []);
    void api.addPin({ number, name: "", electrical_type: "input", shape: "line", at: { x: 0, y: 0 }, angle_deg: 180, length_mm: 2.54, unit: state.activeUnit, body_style: state.activeBodyStyle, hidden: false, name_size_mm: 1.27, number_size_mm: 1.27 });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 860, maxWidth: "95vw" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Pin Table</span>
          <span>{state.symbol?.lib_id}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "70vh", overflowY: "auto" }}>
          <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
            <thead>
              <tr>
                {["Number", "Name", "Electrical Type", "Graphic Style", "Orientation", "X (mm)", "Y (mm)", "Length (mm)", "Unit", "Style", "Visible", ""].map((h) => (
                  <th key={h} style={{ textAlign: "left", padding: "2px 6px", borderBottom: "1px solid var(--chrome-border, #333)" }}>
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {pins.map((p) => (
                <Row key={p.id} pin={p} />
              ))}
            </tbody>
          </table>
          {pins.length === 0 && <p style={{ color: "var(--chrome-text-dim)" }}>No pins yet.</p>}
        </div>
        <div className="dialog-footer">
          <button onClick={addPin}>Add Pin</button>
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
