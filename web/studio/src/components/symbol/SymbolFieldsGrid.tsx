// The fields grid of Symbol Properties (`FIELDS_GRID_TABLE` in `DIALOG_LIB_SYMBOL_PROPERTIES`): one row per field -- the five mandatory ones, then the user fields, then (for a
// derived symbol) the parent's fields it inherits, shown in italics and grey -- with KiCad's visible columns Name, Value, Show, Show Name, H Align, V Align, Italic, Bold
// (`ShowHideColumns( "0 1 2 3 4 5 6 7" )`) and, behind "More columns", the ones KiCad hides: text size, orientation, position and autoplacement. The buttons are the dialog's own:
// add a field (`OnAddField`: a hidden "Field<n>" the name of which is selected), delete (`OnDeleteField`: "The first 5 fields are mandatory."; an inherited field is reset to
// its parent's value instead, `EraseRow`), move up and down (`OnMoveUp` / `OnMoveDown`, never into the mandatory rows). The rows' rules live in kicad-port/symFieldsGrid.ts.
import { useState } from "react";
import type { FieldHAlign, FieldVAlign } from "../../api/types";
import { MANDATORY_FIELDS, isMandatoryField } from "../../kicad-port/symFields";
import { moveRow, newUserField, type GridRow } from "../../kicad-port/symFieldsGrid";

const H_ALIGNS: FieldHAlign[] = ["left", "center", "right"];
const V_ALIGNS: FieldVAlign[] = ["top", "center", "bottom"];
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

export interface SymbolFieldsGridProps {
  rows: readonly GridRow[];
  onChange: (rows: GridRow[]) => void;
  /** The name of the parent symbol, for the tooltip of an inherited row. */
  parentName?: string;
  /** A message the grid wants shown (the dialog's error line). */
  onError: (message: string | null) => void;
}

export function SymbolFieldsGrid({ rows, onChange, parentName, onError }: SymbolFieldsGridProps) {
  const [selected, setSelected] = useState<number | null>(null);
  const [more, setMore] = useState(false);
  const nextKey = () => rows.reduce((m, r) => Math.max(m, r.key), 0) + 1;
  const edit = (index: number, patch: Partial<GridRow>) => onChange(rows.map((r, i) => (i === index ? { ...r, ...patch } : r)));

  const add = () => {
    onChange([...rows, newUserField(rows.length, nextKey())]);
    setSelected(rows.length);
    onError(null);
  };
  const remove = () => {
    if (selected === null) return;
    const row = rows[selected];
    if (!row) return;
    if (selected < MANDATORY_FIELDS.length) return onError(`The first ${MANDATORY_FIELDS.length} fields are mandatory.`);
    if (row.inherited && row.parent) {
      // "You can't erase inherited fields, but you can reset them to the parent value."
      edit(selected, { ...row.parent });
      return;
    }
    onChange(rows.filter((_, i) => i !== selected));
    setSelected(null);
    onError(null);
  };
  const move = (dir: -1 | 1) => {
    if (selected === null) return;
    const next = moveRow(rows, selected, dir);
    if (next === rows) return;
    onChange([...next]);
    setSelected(selected + dir);
  };

  const num = (value: number, onValue: (v: number) => void, label: string, width = 58) => (
    <input type="number" step="any" value={value} aria-label={label} style={{ width }} onChange={(e) => Number.isFinite(Number(e.target.value)) && onValue(Number(e.target.value))} />
  );

  return (
    <div data-testid="symbol-fields-grid">
      <div style={{ display: "flex", gap: 4, marginBottom: 4, alignItems: "center" }}>
        <button onClick={add} title="Add field" aria-label="Add field" data-testid="field-add">
          +
        </button>
        <button onClick={() => move(-1)} title="Move up" aria-label="Move field up" data-testid="field-up">
          ▲
        </button>
        <button onClick={() => move(1)} title="Move down" aria-label="Move field down" data-testid="field-down">
          ▼
        </button>
        <button onClick={remove} title="Delete field" aria-label="Delete field" data-testid="field-delete">
          ✕
        </button>
        <label style={{ marginLeft: "auto", fontSize: 11 }}>
          <input type="checkbox" checked={more} onChange={(e) => setMore(e.target.checked)} data-testid="fields-more" /> More columns
        </label>
      </div>
      <div style={{ maxHeight: 220, overflow: "auto", border: "1px solid var(--chrome-border)" }}>
        <table className="setup-table" style={{ margin: 0, width: "100%" }}>
          <thead>
            <tr>
              <th>Name</th>
              <th>Value</th>
              <th>Show</th>
              <th>Show Name</th>
              <th>H Align</th>
              <th>V Align</th>
              <th>Italic</th>
              <th>Bold</th>
              {more && (
                <>
                  <th>Text Size</th>
                  <th>Orientation</th>
                  <th>X</th>
                  <th>Y</th>
                  <th>Autoplace</th>
                </>
              )}
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const mandatory = isMandatoryField(r.name) && i < MANDATORY_FIELDS.length;
              const grey = r.inherited && r.parent ? { color: "var(--chrome-text-dim)", fontStyle: "italic" as const } : undefined;
              return (
                <tr
                  key={r.key}
                  data-testid={`field-row-${i}`}
                  data-field={r.name}
                  data-inherited={r.inherited ? "true" : "false"}
                  aria-selected={selected === i}
                  title={r.inherited ? `This field is inherited from '${parentName ?? "the parent symbol"}'.` : undefined}
                  onClick={() => setSelected(i)}
                  style={{ background: selected === i ? "var(--chrome-selected-bg)" : undefined }}
                >
                  <td>
                    <input value={r.name} readOnly={mandatory} disabled={mandatory} aria-label={`Name of field ${i + 1}`} data-testid={`field-name-${i}`} style={{ width: 96, ...grey }} onChange={(e) => edit(i, { name: e.target.value })} />
                  </td>
                  <td>
                    <input value={r.text} aria-label={`Value of ${r.name}`} data-testid={`field-text-${i}`} style={{ width: 150, ...grey }} onChange={(e) => edit(i, { text: e.target.value })} />
                  </td>
                  <td style={{ textAlign: "center" }}>
                    <input type="checkbox" checked={!r.hidden} aria-label={`Show ${r.name}`} data-testid={`field-show-${i}`} onChange={(e) => edit(i, { hidden: !e.target.checked })} />
                  </td>
                  <td style={{ textAlign: "center" }}>
                    <input type="checkbox" checked={r.show_name} aria-label={`Show the name of ${r.name}`} data-testid={`field-show-name-${i}`} onChange={(e) => edit(i, { show_name: e.target.checked })} />
                  </td>
                  <td>
                    <select value={r.h_align} aria-label={`Horizontal alignment of ${r.name}`} data-testid={`field-halign-${i}`} onChange={(e) => edit(i, { h_align: e.target.value as FieldHAlign })}>
                      {H_ALIGNS.map((a) => (
                        <option key={a} value={a}>
                          {cap(a)}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <select value={r.v_align} aria-label={`Vertical alignment of ${r.name}`} data-testid={`field-valign-${i}`} onChange={(e) => edit(i, { v_align: e.target.value as FieldVAlign })}>
                      {V_ALIGNS.map((a) => (
                        <option key={a} value={a}>
                          {cap(a)}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td style={{ textAlign: "center" }}>
                    <input type="checkbox" checked={r.italic} aria-label={`${r.name} italic`} data-testid={`field-italic-${i}`} onChange={(e) => edit(i, { italic: e.target.checked })} />
                  </td>
                  <td style={{ textAlign: "center" }}>
                    <input type="checkbox" checked={r.bold} aria-label={`${r.name} bold`} data-testid={`field-bold-${i}`} onChange={(e) => edit(i, { bold: e.target.checked })} />
                  </td>
                  {more && (
                    <>
                      <td>{num(r.size_mm, (v) => v > 0 && edit(i, { size_mm: v }), `Text size of ${r.name}`)}</td>
                      <td>
                        <select value={r.angle_deg % 180 === 0 ? 0 : r.angle_deg === 90 || r.angle_deg === 270 ? 90 : r.angle_deg} aria-label={`Orientation of ${r.name}`} onChange={(e) => edit(i, { angle_deg: Number(e.target.value) })}>
                          <option value={0}>Horizontal</option>
                          <option value={90}>Vertical</option>
                        </select>
                      </td>
                      <td>{num(r.at.x, (v) => edit(i, { at: { x: v, y: r.at.y } }), `X of ${r.name}`)}</td>
                      <td>{num(r.at.y, (v) => edit(i, { at: { x: r.at.x, y: v } }), `Y of ${r.name}`)}</td>
                      <td style={{ textAlign: "center" }}>
                        <input type="checkbox" checked={!r.do_not_autoplace} aria-label={`Allow autoplacement of ${r.name}`} onChange={(e) => edit(i, { do_not_autoplace: !e.target.checked })} />
                      </td>
                    </>
                  )}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
