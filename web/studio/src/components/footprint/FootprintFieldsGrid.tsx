// The fields grid of the Footprint Editor's Footprint Properties (`PCB_FIELDS_GRID_TABLE` in `DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR`): the Reference, the Value, the Datasheet and
// the Description (their texts and places are the footprint's own), then the user fields, each with KiCad's columns -- Name, Value, Show, Width, Height, Thickness, Layer, and behind "More columns" Italic, Bold,
// Orientation, Keep Upright, X Offset, Y Offset, Knockout, Mirrored and the justification. The buttons are the dialog's: add a field (`OnAddField`: hidden, at the origin, on the
// fabrication layer, named `Field<n>`), delete (`OnDeleteField`: "The first 4 fields are mandatory."), move up and down (`OnMoveUp` / `OnMoveDown`, never into the mandatory rows).
// The rows' rules live in kicad-port/fpLibFields.ts.
import { useState } from "react";
import type { FieldLayoutCmd } from "../../api/types";
import { MANDATORY_ROWS, moveRow, newFieldLayout, newRowName, type LibFieldRow } from "../../kicad-port/fpLibFields";
import { layerDisplayName } from "../../kicad-port/boardLayers";
import type { LayerSetup } from "../../api/types";
import { umFrom, umTo, type LengthUnit } from "../../state/units";

export interface FootprintFieldsGridProps {
  rows: readonly LibFieldRow[];
  onChange: (rows: LibFieldRow[]) => void;
  units: LengthUnit;
  /** The layers the board has (Board Setup > Board Editor Layers), for the Layer column. */
  layers: readonly string[];
  layerSetup: LayerSetup | null | undefined;
  onError: (message: string | null) => void;
}

const cell = { padding: "1px 3px" } as const;
const norm360 = (a: number): number => ((a % 360) + 360) % 360;

export function FootprintFieldsGrid({ rows, onChange, units, layers, layerSetup, onError }: FootprintFieldsGridProps) {
  const [selected, setSelected] = useState(0);
  const [more, setMore] = useState(false);
  const setRow = (i: number, patch: Partial<LibFieldRow>) => onChange(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const setLayout = (i: number, patch: Partial<FieldLayoutCmd>) => onChange(rows.map((r, j) => (j === i ? { ...r, layout: { ...r.layout, ...patch } } : r)));
  const num = (v: string, fallback: number): number => (Number.isFinite(Number(v)) && v.trim() !== "" ? Number(v) : fallback);
  /** A length cell: the value in the unit on show, written back in um. */
  const lengthCell = (um: number, onValue: (um: number) => void, label: string, width = 62) => (
    <input type="number" step="any" aria-label={label} value={Number(umTo(um, units).toFixed(4))} onChange={(e) => onValue(Math.round(umFrom(num(e.target.value, umTo(um, units)), units)))} style={{ width }} />
  );

  const add = () => {
    const key = rows.reduce((m, r) => Math.max(m, r.key), 0) + 1;
    onChange([...rows, { key, name: newRowName(rows), text: "", layout: newFieldLayout() }]);
    setSelected(rows.length);
    onError(null);
  };
  const remove = () => {
    if (selected < MANDATORY_ROWS) return onError(`The first ${MANDATORY_ROWS} fields are mandatory.`);
    if (!rows[selected]) return;
    onChange(rows.filter((_, i) => i !== selected));
    setSelected(Math.max(0, selected - 1));
    onError(null);
  };
  const move = (dir: -1 | 1) => {
    const next = moveRow(rows, selected, dir);
    if (next === rows) return;
    onChange([...next]);
    setSelected(selected + dir);
  };

  return (
    <div data-testid="fp-lib-fields-grid">
      <div style={{ display: "flex", gap: 4, marginBottom: 4, alignItems: "center" }}>
        <button onClick={add} title="Add field" aria-label="Add field" data-testid="lib-field-add">
          +
        </button>
        <button onClick={() => move(-1)} title="Move up" aria-label="Move field up" data-testid="lib-field-up">
          ▲
        </button>
        <button onClick={() => move(1)} title="Move down" aria-label="Move field down" data-testid="lib-field-down">
          ▼
        </button>
        <button onClick={remove} title="Delete field" aria-label="Delete field" data-testid="lib-field-delete">
          ✕
        </button>
        <label style={{ marginLeft: "auto", fontSize: 11 }}>
          <input type="checkbox" checked={more} onChange={(e) => setMore(e.target.checked)} data-testid="lib-fields-more" /> More columns
        </label>
      </div>
      <div style={{ overflowX: "auto", border: "1px solid var(--chrome-border)", maxHeight: 220, overflowY: "auto" }}>
        <table style={{ borderCollapse: "collapse", fontSize: 12 }}>
          <thead>
            <tr style={{ textAlign: "left" }}>
              <th style={cell}>Name</th>
              <th style={cell}>Value</th>
              <th style={cell}>Show</th>
              <th style={cell}>Width</th>
              <th style={cell}>Height</th>
              <th style={cell}>Thickness</th>
              <th style={cell}>Layer</th>
              {more && (
                <>
                  <th style={cell}>Italic</th>
                  <th style={cell}>Bold</th>
                  <th style={cell}>Orientation</th>
                  <th style={cell}>Keep Upright</th>
                  <th style={cell}>X Offset</th>
                  <th style={cell}>Y Offset</th>
                  <th style={cell}>Knockout</th>
                  <th style={cell}>Mirrored</th>
                  <th style={cell}>H</th>
                  <th style={cell}>V</th>
                </>
              )}
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const mandatory = i < MANDATORY_ROWS;
              const angle = norm360((r.layout.angle ?? 0) / 1000);
              return (
                <tr key={r.key} onClick={() => setSelected(i)} style={{ background: i === selected ? "var(--chrome-hover, rgba(128,128,128,.18))" : undefined }} data-testid={`lib-field-row-${i}`} data-field={r.name}>
                  <td style={cell}>
                    <input value={r.name} disabled={mandatory} aria-label={`Name of field ${i + 1}`} data-testid={`lib-field-name-${i}`} onChange={(e) => setRow(i, { name: e.target.value })} style={{ width: 110 }} />
                  </td>
                  <td style={cell}>
                    <input value={r.text} aria-label={`Value of ${r.name}`} data-testid={`lib-field-text-${i}`} onChange={(e) => setRow(i, { text: e.target.value })} style={{ width: 130 }} />
                  </td>
                  <td style={cell}>
                    <input type="checkbox" checked={r.layout.visible !== false} aria-label={`Show ${r.name}`} data-testid={`lib-field-show-${i}`} onChange={(e) => setLayout(i, { visible: e.target.checked })} />
                  </td>
                  <td style={cell}>{lengthCell(r.layout.size[0], (w) => setLayout(i, { size: [w, r.layout.size[1]] }), `Width of ${r.name}`)}</td>
                  <td style={cell}>{lengthCell(r.layout.size[1], (h) => setLayout(i, { size: [r.layout.size[0], h] }), `Height of ${r.name}`)}</td>
                  <td style={cell}>{lengthCell(r.layout.thickness ?? 0, (t) => setLayout(i, { thickness: t }), `Thickness of ${r.name}`)}</td>
                  <td style={cell}>
                    <select value={r.layout.layer} aria-label={`Layer of ${r.name}`} data-testid={`lib-field-layer-${i}`} onChange={(e) => setLayout(i, { layer: e.target.value })} style={{ width: 92 }}>
                      {(layers.includes(r.layout.layer) ? layers : [...layers, r.layout.layer]).map((l) => (
                        <option key={l} value={l}>
                          {layerDisplayName(layerSetup, l)}
                        </option>
                      ))}
                    </select>
                  </td>
                  {more && (
                    <>
                      <td style={cell}>
                        <input type="checkbox" checked={!!r.layout.italic} aria-label={`${r.name} italic`} onChange={(e) => setLayout(i, { italic: e.target.checked })} />
                      </td>
                      <td style={cell}>
                        <input type="checkbox" checked={!!r.layout.bold} aria-label={`${r.name} bold`} data-testid={`lib-field-bold-${i}`} onChange={(e) => setLayout(i, { bold: e.target.checked })} />
                      </td>
                      <td style={cell}>
                        <input type="number" step="any" aria-label={`Orientation of ${r.name}`} value={Number(angle.toFixed(3))} onChange={(e) => setLayout(i, { angle: Math.round(norm360(num(e.target.value, angle)) * 1000) })} style={{ width: 58 }} />
                      </td>
                      <td style={cell}>
                        <input type="checkbox" checked={r.layout.keep_upright !== false} aria-label={`${r.name} keep upright`} onChange={(e) => setLayout(i, { keep_upright: e.target.checked })} />
                      </td>
                      <td style={cell}>{lengthCell(r.layout.at.x, (v) => setLayout(i, { at: { ...r.layout.at, x: v } }), `X offset of ${r.name}`)}</td>
                      <td style={cell}>{lengthCell(r.layout.at.y, (v) => setLayout(i, { at: { ...r.layout.at, y: v } }), `Y offset of ${r.name}`)}</td>
                      <td style={cell}>
                        <input type="checkbox" checked={!!r.layout.knockout} aria-label={`${r.name} knockout`} onChange={(e) => setLayout(i, { knockout: e.target.checked })} />
                      </td>
                      <td style={cell}>
                        <input type="checkbox" checked={!!r.layout.mirror} aria-label={`${r.name} mirrored`} onChange={(e) => setLayout(i, { mirror: e.target.checked })} />
                      </td>
                      <td style={cell}>
                        <select value={r.layout.halign ?? 0} aria-label={`Horizontal justification of ${r.name}`} onChange={(e) => setLayout(i, { halign: Number(e.target.value) as -1 | 0 | 1 })}>
                          <option value={-1}>Left</option>
                          <option value={0}>Center</option>
                          <option value={1}>Right</option>
                        </select>
                      </td>
                      <td style={cell}>
                        <select value={r.layout.valign ?? 0} aria-label={`Vertical justification of ${r.name}`} onChange={(e) => setLayout(i, { valign: Number(e.target.value) as -1 | 0 | 1 })}>
                          <option value={-1}>Top</option>
                          <option value={0}>Center</option>
                          <option value={1}>Bottom</option>
                        </select>
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
