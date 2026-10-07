// Port of `DIALOG_FP_EDIT_PAD_TABLE` (pcbnew/dialogs/dialog_fp_edit_pad_table.cpp), opened by `pcbnew.ModuleEditor.padTable` ("Pad Table...":
// "Displays pad table for bulk editing of pads"): one row per pad of the open footprint -- number, type, shape, position, size, drill -- all
// editable, above the "Pad numbers: 1-8 / Pad count / Duplicate pads" summary (`PIN_NUMBERS`). OK applies every changed row as ONE undo step
// ("Edit Pads"); Cancel leaves the footprint untouched (the C++ edits live and restores on cancel; here nothing is applied until OK).
// A row that gets focus selects its pad on the canvas (`OnSelectCell`, `SetBrightened`).
//
// Not ported: the Connector / Aperture pad types (the footprint IR has plated, SMD and non-plated pads), a custom pad shape, and the "Pad->Die
// Length / Delay" columns (the IR carries no pad-to-die data).
import { useEffect, useMemo, useState } from "react";
import type { LibraryPad, LibraryPadShape, PadKind } from "../../api/types";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { padNumberSummary } from "../../kicad-port/pinNumbersSummary";
import { drillColumns, withDrill } from "../../kicad-port/padTable";
import { umFrom, umTo, type LengthUnit } from "../../state/units";

const TYPE_OPTIONS: { value: PadKind; label: string }[] = [
  { value: "through_hole", label: "Through-hole" },
  { value: "smd", label: "SMD" },
  { value: "non_plated_hole", label: "NPTH" },
];

const SHAPE_OPTIONS: { value: LibraryPadShape; label: string }[] = [
  { value: "circle", label: "Circle" },
  { value: "oval", label: "Oval" },
  { value: "rect", label: "Rectangle" },
  { value: "trapezoid", label: "Trapezoid" },
  { value: "round_rect", label: "Rounded rectangle" },
  { value: "chamfered_rect", label: "Chamfered rectangle" },
];

/** A length cell that keeps what is being typed until it is committed (so "1." and "-" survive), and shows the value again when it changes underneath. */
function LengthCell({ valueUm, units, disabled, onCommit, label }: { valueUm: number; units: LengthUnit; disabled?: boolean; onCommit: (um: number) => void; label: string }) {
  const shown = String(umTo(valueUm, units));
  const [text, setText] = useState(shown);
  const [focused, setFocused] = useState(false);
  useEffect(() => {
    if (!focused) setText(shown);
  }, [shown, focused]);
  const commit = (t: string) => {
    const n = Number(t);
    if (t.trim() !== "" && Number.isFinite(n)) onCommit(Math.round(umFrom(n, units)));
  };
  return (
    <input
      aria-label={label}
      value={text}
      disabled={disabled}
      style={{ width: 70 }}
      onFocus={() => setFocused(true)}
      onBlur={() => {
        setFocused(false);
        commit(text);
        setText(shown);
      }}
      onChange={(e) => {
        setText(e.target.value);
        commit(e.target.value);
      }}
    />
  );
}

export function PadTableDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const units = useStudioState().units;
  const open = state.padTableOpen;
  const [rows, setRows] = useState<LibraryPad[]>([]);

  useEffect(() => {
    if (open && state.footprint) setRows(state.footprint.pads.map((p) => ({ ...p })));
    // The table works on the pads as they were when it opened (`CaptureOriginalPadState`).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const summary = useMemo(() => padNumberSummary(rows.map((r) => r.number)), [rows]);
  if (!open || !state.footprint) return null;
  const close = () => dispatch({ type: "SET_PAD_TABLE_OPEN", open: false });
  const original = new Map(state.footprint.pads.map((p) => [p.id, p]));
  const patch = (i: number, f: (p: LibraryPad) => LibraryPad) => setRows((rs) => rs.map((r, j) => (j === i ? f(r) : r)));

  const apply = async () => {
    const changed = rows.filter((r) => JSON.stringify(r) !== JSON.stringify(original.get(r.id)));
    if (changed.length > 0 && state.name) {
      const cmds = changed.map((pad) => ({ op: "edit_pad" as const, footprint: state.name!, id: pad.id!, pad }));
      if (!(await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds }))) return; // the verb's refusal is already a toast; the table stays open
    }
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 760, maxHeight: "80vh" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Pad Table">
        <div className="dialog-header">
          <span>Pad Table</span>
          <span style={{ fontWeight: 400, color: "var(--chrome-text-dim)" }}>{state.name}</span>
        </div>
        <div style={{ display: "flex", gap: 20, padding: "8px 16px", fontSize: 12 }}>
          <span>
            Pad numbers: <strong>{summary.summary || "0"}</strong>
          </span>
          <span>
            Pad count: <strong>{rows.length}</strong>
          </span>
          <span>
            Duplicate pads: <strong>{summary.duplicates}</strong>
          </span>
        </div>
        <div className="dialog-body" style={{ padding: "0 16px 8px", overflow: "auto" }}>
          <table className="setup-table" style={{ margin: 0 }}>
            <thead>
              <tr>
                <th>Number</th>
                <th>Type</th>
                <th>Shape</th>
                <th>X Position</th>
                <th>Y Position</th>
                <th>Size X</th>
                <th>Size Y</th>
                <th>Drill X</th>
                <th>Drill Y</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => {
                const [dx, dy] = drillColumns(r);
                const drillable = r.kind !== "smd";
                return (
                  <tr key={r.id ?? i} onFocus={() => r.id && dispatch({ type: "SET_SELECTION", refs: [r.id] })}>
                    <td>
                      <input aria-label={`Number ${i + 1}`} value={r.number} style={{ width: 56 }} onChange={(e) => patch(i, (p) => ({ ...p, number: e.target.value }))} />
                    </td>
                    <td>
                      <select
                        aria-label={`Type ${i + 1}`}
                        value={r.kind}
                        onChange={(e) =>
                          patch(i, (p) => {
                            const kind = e.target.value as PadKind;
                            // `SetAttribute`: an SMD pad has no hole; a pad that becomes plated or non-plated needs one.
                            return kind === "smd" ? { ...p, kind, drill: null, drill_slot: null } : { ...p, kind, drill: p.drill ?? (p.drill_slot ? null : 800) };
                          })
                        }
                      >
                        {TYPE_OPTIONS.map((o) => (
                          <option key={o.value} value={o.value}>
                            {o.label}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <select aria-label={`Shape ${i + 1}`} value={r.shape} onChange={(e) => patch(i, (p) => ({ ...p, shape: e.target.value as LibraryPadShape }))}>
                        {SHAPE_OPTIONS.map((o) => (
                          <option key={o.value} value={o.value}>
                            {o.label}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <LengthCell label={`X ${i + 1}`} units={units} valueUm={r.at.x} onCommit={(x) => patch(i, (p) => ({ ...p, at: { ...p.at, x } }))} />
                    </td>
                    <td>
                      <LengthCell label={`Y ${i + 1}`} units={units} valueUm={r.at.y} onCommit={(y) => patch(i, (p) => ({ ...p, at: { ...p.at, y } }))} />
                    </td>
                    <td>
                      <LengthCell label={`Size X ${i + 1}`} units={units} valueUm={r.size[0]} onCommit={(w) => patch(i, (p) => ({ ...p, size: [w, p.size[1]] }))} />
                    </td>
                    <td>
                      <LengthCell label={`Size Y ${i + 1}`} units={units} valueUm={r.size[1]} onCommit={(h) => patch(i, (p) => ({ ...p, size: [p.size[0], h] }))} />
                    </td>
                    <td>
                      <LengthCell label={`Drill X ${i + 1}`} units={units} valueUm={dx} disabled={!drillable} onCommit={(v) => patch(i, (p) => withDrill(p, v, drillColumns(p)[1]))} />
                    </td>
                    <td>
                      <LengthCell label={`Drill Y ${i + 1}`} units={units} valueUm={dy} disabled={!drillable} onCommit={(v) => patch(i, (p) => withDrill(p, drillColumns(p)[0], v))} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "8px 0 0" }}>Lengths in {units}. Pad-to-die length and delay are not modeled.</p>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={() => void apply()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
