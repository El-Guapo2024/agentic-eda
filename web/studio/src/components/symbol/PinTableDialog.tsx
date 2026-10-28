// Port of eeschema/dialogs/dialog_lib_edit_pin_table.cpp and `SYMBOL_EDITOR_EDIT_TOOL::PinTable` -- one row per pin (KiCad's own row can merge
// several identical pins into one; this app's one-physical-pin-per-row model skips that grouping, a documented simplification, see
// PARITY-symedit.md), every field inline-editable, sorted by (unit, number).
//
// As KiCad's, the table edits copies of the pins (`TransferDataToWindow`): OK writes them to the symbol as one undo step ("Edit Pins",
// kicad-port/pinTable.ts `pinTableCommands`), Cancel or closing with changes asks "Save changes?" (`OnClose`'s `HandleUnsavedChanges`), and
// on a derived symbol OK is "Read Only". The rows act on their own pins only: Synchronized Pins Mode does not reach the table (`UpdatePin`
// never calls `SynchronizePins`). Unit and Body Style are KiCad's combo boxes, a length edit is `ChangeLength` (the body end stays), Y is
// KiCad's internal Y (the file's negated), and Add Pin is `OnAddRow` (kicad-port/pinTable.ts).
//
// Not ported: "Group by name", "Group Selected Pins", column-header sort, the unit / body style filters, the summary line, the Count and text
// size columns, and CSV/clipboard import-export; see PARITY-symedit.md.
import { useEffect, useState } from "react";
import type { LibrarySymbolPin, PinElectricalType, PinShape } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { angleDegToOrientation, orientationToAngleDeg, PIN_ORIENTATIONS, type PinOrientation } from "../../kicad-port/pinOrientation";
import { addedPin, bodyStyleCellEditable, bodyStyleChoices, displayY, pinTableCommands, storedY, unitCellEditable, unitChoices } from "../../kicad-port/pinTable";
import { changePinLength } from "../../kicad-port/symPinSync";
import { askChoice } from "../library/libraryDialogs";

const ELECTRICAL_TYPES: PinElectricalType[] = ["input", "output", "bidirectional", "tri_state", "passive", "free", "unspecified", "power_in", "power_out", "open_collector", "open_emitter", "no_connect"];
const SHAPES: PinShape[] = ["line", "inverted", "clock", "inverted_clock", "input_low", "clock_low", "output_low", "edge_clock_high", "non_logic"];
const ORIENTATION_LABELS: Record<PinOrientation, string> = { right: "Right", left: "Left", up: "Up", down: "Down" };

/** A number cell: typed freely, taken on blur. */
function NumberCell({ value, onCommit, width, label }: { value: number; onCommit: (v: number) => void; width: number; label: string }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  return (
    <input
      type="number"
      step="any"
      value={text}
      aria-label={label}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const v = Number(text);
        if (text.trim() !== "" && Number.isFinite(v) && v !== value) onCommit(v);
        else setText(String(value));
      }}
      style={{ width }}
    />
  );
}

export function PinTableDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const sym = state.symbol;
  /** The copies the table edits; `null` until it opens. */
  const [rows, setRows] = useState<LibrarySymbolPin[] | null>(null);
  const [before, setBefore] = useState<LibrarySymbolPin[]>([]);
  const [edited, setEdited] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!state.pinTableOpen) {
      setRows(null);
      return;
    }
    const pins = [...(api.getState().symbol?.pins ?? [])].sort((a, b) => a.unit - b.unit || a.number.localeCompare(b.number, undefined, { numeric: true }));
    setBefore(pins);
    setRows(pins.map((p) => ({ ...p })));
    setEdited(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.pinTableOpen]);

  if (!state.pinTableOpen || !sym || !rows) return null;
  /** `!IsSymbolEditable() || IsSymbolAlias()`: a derived symbol's pins are its parent's. */
  const readOnly = state.derivedOwn !== null;
  const close = () => dispatch({ type: "SET_PIN_TABLE_OPEN", open: false });

  const update = (i: number, next: LibrarySymbolPin) => {
    setRows((rs) => (rs ? rs.map((r, j) => (j === i ? next : r)) : rs));
    setEdited(true);
  };

  /** `TransferDataFromWindow` then `commit.Push( _( "Edit Pins" ) )`. */
  const save = async (): Promise<boolean> => {
    if (readOnly || !state.libId) return false;
    const cmds = pinTableCommands(state.libId, before, rows);
    if (cmds.length === 0) return true;
    setBusy(true);
    try {
      return await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    } finally {
      setBusy(false);
    }
  };

  const ok = async () => {
    if (await save()) close();
  };

  /** `OnClose`: with changes, "Save changes?" -- Save, Discard Changes, or Cancel (the table stays open). */
  const cancel = async () => {
    if (edited && !readOnly) {
      const answer = await askChoice({
        title: "Save Changes?",
        message: "Save changes?\n\nIf you don't save, all your changes will be permanently lost.",
        okLabel: "Save",
        altLabel: "Discard Changes",
        cancelLabel: "Cancel",
      });
      if (answer === null) return;
      if (answer === "ok" && !(await save())) return;
    }
    close();
  };

  const units = unitChoices(sym);
  const styles = bodyStyleChoices(sym);
  const unitEditable = unitCellEditable(sym);
  const styleEditable = bodyStyleCellEditable(sym);

  return (
    <div className="dialog-backdrop" onClick={() => void cancel()}>
      <div className="dialog" style={{ width: 900, maxWidth: "95vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Pin Table" data-testid="pin-table">
        <div className="dialog-header">
          <span>Pin Table</span>
          <span>{sym.lib_id}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "70vh", overflowY: "auto" }}>
          <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
            <thead>
              <tr>
                {["Number", "Name", "Electrical Type", "Graphic Style", "Orientation", "X Position", "Y Position", "Length", "Unit", "Body Style", "Visible", ""].map((h) => (
                  <th key={h} style={{ textAlign: "left", padding: "2px 6px", borderBottom: "1px solid var(--chrome-border, #333)" }}>
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((p, i) => (
                <tr key={p.id ?? `new-${i}`} data-testid="pin-table-row">
                  <td>
                    <input value={p.number} aria-label="Number" onChange={(e) => update(i, { ...p, number: e.target.value })} style={{ width: 50 }} />
                  </td>
                  <td>
                    <input value={p.name} aria-label="Name" onChange={(e) => update(i, { ...p, name: e.target.value })} style={{ width: 70 }} />
                  </td>
                  <td>
                    <select value={p.electrical_type} aria-label="Electrical Type" onChange={(e) => update(i, { ...p, electrical_type: e.target.value as PinElectricalType })}>
                      {ELECTRICAL_TYPES.map((t) => (
                        <option key={t} value={t}>
                          {t}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <select value={p.shape} aria-label="Graphic Style" onChange={(e) => update(i, { ...p, shape: e.target.value as PinShape })}>
                      {SHAPES.map((s) => (
                        <option key={s} value={s}>
                          {s}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <select value={angleDegToOrientation(p.angle_deg)} aria-label="Orientation" onChange={(e) => update(i, { ...p, angle_deg: orientationToAngleDeg(e.target.value as PinOrientation) })}>
                      {PIN_ORIENTATIONS.map((o) => (
                        <option key={o} value={o}>
                          {ORIENTATION_LABELS[o]}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <NumberCell label="X Position" value={p.at.x} width={60} onCommit={(x) => update(i, { ...p, at: { ...p.at, x } })} />
                  </td>
                  <td>
                    <NumberCell label="Y Position" value={displayY(p.at.y)} width={60} onCommit={(y) => update(i, { ...p, at: { ...p.at, y: storedY(y) } })} />
                  </td>
                  <td>
                    <NumberCell label="Length" value={p.length_mm} width={55} onCommit={(len) => update(i, changePinLength(p, len))} />
                  </td>
                  <td>
                    <select value={unitEditable ? p.unit : 0} disabled={!unitEditable} aria-label="Unit" onChange={(e) => update(i, { ...p, unit: Number(e.target.value) })}>
                      {(unitEditable ? units : units.slice(0, 1)).map((c) => (
                        <option key={c.value} value={c.value}>
                          {c.label}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <select value={styleEditable ? p.body_style : 0} disabled={!styleEditable} aria-label="Body Style" onChange={(e) => update(i, { ...p, body_style: Number(e.target.value) })}>
                      {styles.map((c) => (
                        <option key={c.value} value={c.value}>
                          {c.label}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <input type="checkbox" aria-label="Visible" checked={!p.hidden} onChange={(e) => update(i, { ...p, hidden: !e.target.checked })} />
                  </td>
                  <td>
                    <button
                      onClick={() => {
                        setRows((rs) => (rs ? rs.filter((_, j) => j !== i) : rs));
                        setEdited(true);
                      }}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length === 0 && <p style={{ color: "var(--chrome-text-dim)" }}>No pins yet.</p>}
        </div>
        <div className="dialog-footer">
          <button
            onClick={() => {
              setRows((rs) => (rs ? [...rs, addedPin(rs)] : rs));
              setEdited(true);
            }}
            disabled={readOnly}
            data-testid="pin-table-add"
          >
            Add Pin
          </button>
          <button onClick={() => void cancel()}>Cancel</button>
          <button className="primary" disabled={readOnly || busy} onClick={() => void ok()} data-testid="pin-table-ok">
            {readOnly ? "Read Only" : "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}
