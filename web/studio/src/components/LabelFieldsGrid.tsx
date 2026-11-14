// The fields grid of Label Properties (`DIALOG_LABEL_PROPERTIES`' `FIELDS_GRID_TABLE`, kicad-port/labelFieldsGrid.ts), for an existing label
// (components/SchPropertiesDialogs.tsx) and a new one (components/LabelDialog.tsx, `createNewLabel`'s dialog): a global label's Sheet References first, then the
// label's own fields; Name .. Bold shown at open (`ShowHideColumns( "0 1 2 3 4 5 6 7" )`), the others from the column labels' right-click; Add Field
// (`OnAddField`) and the x of a row. A new label's rows have no place yet (`placeless`): it is placed by the click and its fields by `AutoplaceFields`, so their
// H / V Align, Orientation and X / Y cells are empty.
import { useRef, useState, type Dispatch, type SetStateAction } from "react";
import { ContextMenu } from "./canvas/ContextMenu";
import { lookCellOf } from "./fieldGridCells";
import { useFontFamilies } from "./TextLookControls";
import { columnMenu, DEFAULT_SHOWN_COLUMNS, FIELD_COLUMNS, shownColumns, toggleColumn, type FieldColumn } from "../kicad-port/symbolFieldsGrid";
import { fieldLookError, fontChoices } from "../kicad-port/symbolPropsDialog";
import { labelFieldEditOf, newLabelGridRow, type LabelGridRow } from "../kicad-port/labelFieldsGrid";
import { labelFieldDisplayName, newLabelField } from "../kicad-port/schProperties";

/** The columns of a field's place, empty on a row that has none yet. */
const PLACE_COLUMNS: ReadonlySet<FieldColumn> = new Set<FieldColumn>(["h_align", "v_align", "orientation", "pos_x", "pos_y"]);

/** Every row's Text Size reads (`FIELDS_GRID_TABLE`'s validator). */
export const labelRowsOk = (rows: readonly LabelGridRow[]): boolean => rows.every((r) => fieldLookError(r.name, r.look) === null);

export function LabelFieldsGrid({ rows, setRows, placeless = false, fontsId = "label-props-fonts" }: { rows: LabelGridRow[]; setRows: Dispatch<SetStateAction<LabelGridRow[]>>; isrVisible?: boolean | null; placeless?: boolean; fontsId?: string }) {
  const nextKey = useRef(1000);
  const [shownCols, setShownCols] = useState<Set<FieldColumn>>(() => shownColumns(DEFAULT_SHOWN_COLUMNS));
  const [columnsMenu, setColumnsMenu] = useState<{ x: number; y: number } | null>(null);
  const fontList = fontChoices(useFontFamilies());
  const own = rows.filter((r) => !r.isr);
  const isrRow = rows.find((r) => r.isr) ?? null;
  const setRow = (key: number, change: Partial<LabelGridRow>) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...change } : r)));
  const addField = () => {
    const f = newLabelField(own.map((r) => ({ ...labelFieldEditOf(r), name: r.name === "Net Class" ? "Netclass" : r.name })), isrRow ? isrRow.visible : null);
    setRows((rs) => [...rs, { ...newLabelGridRow(nextKey.current++, f), name: labelFieldDisplayName(f.name) }]);
  };
  const deleteField = (key: number) => setRows((rs) => rs.filter((r) => r.key !== key));
  return (
    <>
      <datalist id={fontsId}>
        {fontList.map((f) => (
          <option key={f} value={f} />
        ))}
      </datalist>
      <div style={{ overflowX: "auto", marginTop: 8 }}>
        <table className="label-fields" aria-label="Label fields" style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead
            // `GRID_TRICKS::onGridLabelRightClick`: a check item per column
            onContextMenu={(e) => {
              e.preventDefault();
              setColumnsMenu({ x: e.clientX, y: e.clientY });
            }}
            title="Right-click to show or hide columns"
          >
            <tr>
              {FIELD_COLUMNS.filter((c) => shownCols.has(c.key)).map((c) => (
                <th key={c.key} data-col={c.key} style={{ textAlign: c.key === "name" || c.key === "value" ? "left" : "center", whiteSpace: "nowrap" }}>
                  {c.label}
                </th>
              ))}
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              // the own fields are "Field 1" .. in the cells' names; the Sheet References by their name
              const n = r.isr ? "Sheet References" : `Field ${own.indexOf(r) + 1}`;
              return (
                <tr key={r.key} data-row={r.isr ? "isr" : n} title={r.isr ? "Intersheet reference visibility is controlled globally from Schematic Setup > General > Formatting" : undefined}>
                  {FIELD_COLUMNS.filter((c) => shownCols.has(c.key)).map((c) => (
                    <td key={c.key} style={c.key === "name" || c.key === "value" ? undefined : { textAlign: "center" }}>
                      {c.key === "name" ? (
                        r.isr ? (
                          <span>Sheet References</span>
                        ) : (
                          <input aria-label={`${n} name`} value={r.name} onChange={(e) => setRow(r.key, { name: e.target.value })} style={{ width: 110 }} />
                        )
                      ) : c.key === "value" ? (
                        r.isr ? (
                          <span>{r.text}</span>
                        ) : (
                          <input aria-label={`${n} value`} value={r.text} onChange={(e) => setRow(r.key, { text: e.target.value })} />
                        )
                      ) : c.key === "shown" ? (
                        <input type="checkbox" aria-label={r.isr ? "Show Sheet References" : `${n} shown`} checked={r.visible} disabled={r.isr} onChange={(e) => setRow(r.key, { visible: e.target.checked })} />
                      ) : placeless && PLACE_COLUMNS.has(c.key) ? null : (
                        lookCellOf(n, c.key, r.look, (patch) => setRow(r.key, { look: { ...r.look, ...patch } }), fontsId)
                      )}
                    </td>
                  ))}
                  <td>
                    {!r.isr && (
                      <button type="button" aria-label={`Delete field ${own.indexOf(r) + 1}`} title="Delete field" onClick={() => deleteField(r.key)}>
                        ✕
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {columnsMenu && (
        <ContextMenu
          x={columnsMenu.x}
          y={columnsMenu.y}
          entries={columnMenu(shownCols).map((c) => ({ label: `${c.checked ? "\u2713" : "\u2003"} ${c.label}`, checked: c.checked, onSelect: () => setShownCols((cols) => toggleColumn(cols, c.key)) }))}
          onClose={() => setColumnsMenu(null)}
        />
      )}
      <button type="button" aria-label="Add field" title="Add field" onClick={addField} style={{ marginTop: 4 }}>
        + Add Field
      </button>
    </>
  );
}
