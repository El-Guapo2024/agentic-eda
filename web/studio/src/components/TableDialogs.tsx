// Ports of pcbnew/dialogs/dialog_table_properties.cpp ("Table Properties") and dialog_tablecell_properties.cpp ("Table Cell Properties"), for the board's tables.
//
// Table Properties is a grid of the cells' text (a table on a back layer is shown from the other side), the layer and the lock, the border (outline, the line under the header,
// width and style) and the separators (rows, columns, width, style). It opens on `Ctrl+E` / Edit Table... and right after Draw Table's second click on the table the tool has just
// sized, which is not on the board until OK ("Draw Table"); Cancel there adds nothing. Table Cell Properties edits the selected cells together: a field they disagree on is shown as
// `<...>` and left alone, and it has the "Edit Table..." button that opens the table's own dialog. A sheet's (eeschema `DIALOG_TABLECELL_PROPERTIES`) has "Text color:" and
// "Background fill:" too, each a swatch or, for cells that disagree, the `-- mixed values --` popup (kicad-port/tables.ts `cellColorEditOf`); the board's has neither.
//
// Not ported: the syntax help window and the text-variable autocomplete of the cell editors (a plain text area is the cell's editor, and `${ROW}`, `${COL}`, `${ADDR}`, `${LAYER}`
// and the board's variables are resolved as they are anywhere else).
import { useEffect, useState, type CSSProperties } from "react";
import { fetchSchematic, fetchState } from "../api/client";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { closeTableCellDialog, closeTableDialog, openTableDialog, useTableDialogs } from "../state/tableDialogs";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { lineThicknessNm } from "../kicad-port/layerDefaults";
import { ColorSwatch } from "./TextLookControls";
import { autoPenFor, cellById, cellColorEditOf, cellDialogValues, cellEditOf, gridColumn, isSelectableCell, schCellAddress, TABLE_LINE_STYLES, tableById, tableDialogValues, tablePatchOf, tablesOf, widthAfterToggle, type CellColor, type CellDialogValues, type TableDialogValues } from "../kicad-port/tables";
import type { TableCellAddr, TableCmdWire, TableLineStyleName } from "../api/tableTypes";
import type { BoardTable } from "../api/types";
import type { DrawnTable } from "../state/tableDialogs";
import { nmFrom, nmTo, type LengthUnit } from "../state/units";

const GRID: CSSProperties = { gridTemplateColumns: "150px 1fr", rowGap: 6 };

/** A length in the display unit, as the field holds it. */
const shown = (um: number, unit: LengthUnit): string => String(Math.round(nmTo(um, unit) * 10_000) / 10_000);

/** A length field that may be "differs" (`INDETERMINATE_STATE`): empty with `<...>` as its placeholder. */
function LengthField(props: { value: number | null; unit: LengthUnit; onChange: (um: number | null) => void; disabled?: boolean; testId?: string }) {
  const { value, unit, onChange, disabled, testId } = props;
  return (
    <input
      type="number"
      step="any"
      data-testid={testId}
      disabled={disabled}
      placeholder="<...>"
      value={value === null ? "" : shown(value, unit)}
      onChange={(e) => onChange(e.target.value === "" ? null : Math.round(nmFrom(Number(e.target.value), unit)))}
    />
  );
}

function StyleSelect(props: { value: TableLineStyleName; disabled?: boolean; onChange: (s: TableLineStyleName) => void; testId?: string }) {
  return (
    <select value={props.value} disabled={props.disabled} data-testid={props.testId} onChange={(e) => props.onChange(e.target.value as TableLineStyleName)}>
      {TABLE_LINE_STYLES.map((s) => (
        <option key={s.value} value={s.value}>
          {s.label}
        </option>
      ))}
    </select>
  );
}

// ------------------------------------------------------------------------------------------------------------------------------ Table Properties

/**
 * What Table Properties needs from the editor it is open in: the board's and the sheet's (`TablePropertiesDialog` below), or the Footprint Editor's
 * (components/footprint/FpItemDialogs.tsx) -- `DIALOG_TABLE_PROPERTIES` is opened by every frame that has tables.
 */
export interface TableHost {
  /** A new value opens the form afresh. */
  openKey: string;
  /** The table being edited, as the editor's state shows it; or the one Draw Table has just sized (`drawn`), which is not added until OK. */
  table?: BoardTable;
  drawn?: DrawnTable;
  /** A sheet's table: rows and columns counted from 0, no layer, no lock. */
  sch: boolean;
  /** The table's lock; null where there is none (a sheet's table, a footprint's). */
  locked: boolean | null;
  layers: string[];
  layerName: (layer: string) => string;
  units: LengthUnit;
  /** `bds.GetLineThickness( layer )`: what a width starts at when switched on. */
  defaultLine: (layer: string) => number;
  /** OK: the dialog's values, one undo step; true when taken. */
  submit: (v: TableDialogValues) => Promise<boolean>;
  close: () => void;
}

/** The board's and the sheet's dialog: `table` / `sch_table` `draw` for the drawn table, `set` for one there is. */
export function TablePropertiesDialog() {
  const dialogs = useTableDialogs();
  const open = dialogs.table;
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const board = state.board;
  // The board's table or the sheet's: the sheet's has no layer and no lock, and its border and separators start at the default width (0).
  const sch = open?.scope === "sch";
  const tables = sch ? { drawings: { tables: state.schematic?.tables ?? [] } } : board;
  const send = (verb: TableCmdWire): Promise<boolean> => api.cmd(sch ? { op: "sch_table", ...verb } : { op: "table", ...verb });
  const table = open?.mode === "edit" ? tableById(tables, open.tableId) : undefined;
  if (!open || (open.mode === "edit" && !table)) return null;
  const textGraphics = board?.board_rules?.text_graphics;
  const layerSetup = board?.board_rules?.layer_setup;
  const host: TableHost = {
    openKey: open.mode === "edit" ? `edit:${open.tableId}` : `draw:${JSON.stringify(open.drawn)}`,
    table,
    drawn: open.mode === "draw" ? open.drawn : undefined,
    sch,
    locked: sch ? null : table ? (board?.locked ?? []).includes(table.id) : false,
    layers: boardLayerList(board?.layers ?? [], layerSetup),
    layerName: (l) => layerDisplayName(layerSetup, l),
    units: state.units,
    defaultLine: (layer) => (sch ? 0 : lineThicknessNm(textGraphics, layer)),
    submit: async (v) => {
      if (open.mode === "draw") {
        const d = open.drawn;
        const before = new Set(tablesOf(tables).map((t) => t.id));
        const patch = tablePatchOf({ cols: d.cols, layer: d.layer }, v);
        const ok = await send({
          verb: "draw",
          layer: sch ? "" : v.layer,
          at: { x: Math.round(d.at[0]), y: Math.round(d.at[1]) },
          cols: d.cols,
          rows: d.rows,
          cell_w_nm: Math.round(d.cellW),
          cell_h_nm: Math.round(d.cellH),
          line_width_nm: Math.round(d.lineWidth),
          locked: v.locked,
          patch,
        });
        if (ok) {
          closeTableDialog();
          // `selectItem( table )`: the table just made. The board as the backend has it now: `stateRef` only catches up on the next render, so it is not in `getState()` yet.
          const after = sch ? await fetchSchematic(state.currentSheetPath).then((s) => ({ drawings: { tables: s.tables ?? [] } })).catch(() => null) : await fetchState().catch(() => null);
          const made = after ? tablesOf(after).find((t) => !before.has(t.id)) : undefined;
          if (made) dispatch({ type: "SET_SELECTION", refs: [made.id] });
        }
        return false; // closed above when it was taken
      }
      if (!table) return false;
      return send({
        verb: "set",
        id: table.id,
        layer: !sch && v.layer !== table.layer ? v.layer : undefined,
        locked: !sch && v.locked !== (board?.locked ?? []).includes(table.id) ? v.locked : undefined,
        patch: tablePatchOf(table, v),
      });
    },
    close: closeTableDialog,
  };
  return <TablePropertiesForm host={host} />;
}

export function TablePropertiesForm({ host }: { host: TableHost }) {
  const { table, drawn, sch } = host;
  const unit: LengthUnit = host.units;
  const [v, setV] = useState<TableDialogValues | null>(null);
  const [busy, setBusy] = useState(false);

  // The dialog's values come from the table (or the one being drawn) each time it opens -- `TransferDataToWindow`.
  useEffect(() => {
    if (table) {
      setV(tableDialogValues(table, host.locked ?? false, host.defaultLine(table.layer)));
    } else if (drawn) {
      const d = drawn;
      setV({
        layer: d.layer,
        locked: false,
        borderExternal: true,
        borderHeader: true,
        borderWidth: d.lineWidth,
        borderStyle: "solid",
        rowSeparators: true,
        colSeparators: true,
        separatorsWidth: d.lineWidth,
        separatorsStyle: "solid",
        grid: Array.from({ length: d.rows }, () => Array.from({ length: d.cols }, () => "")),
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host.openKey]);

  if (!v) return null;

  const cols = table?.cols ?? drawn!.cols;
  const rows = table?.rows ?? drawn!.rows;
  const layers = host.layers;
  const defaultLine = host.defaultLine(v.layer);
  const set = (patch: Partial<TableDialogValues>) => setV((cur) => (cur ? { ...cur, ...patch } : cur));
  const borderOn = v.borderExternal || v.borderHeader;
  const separatorsOn = v.rowSeparators || v.colSeparators;
  /** A cell another one covers is greyed and cannot be edited. */
  const covered = (row: number, gcol: number): boolean => {
    if (!table) return false;
    const c = table.cells[row * table.cols + gridColumn(table, gcol)];
    return !c || !isSelectableCell(c);
  };

  const toggleBorder = (patch: Partial<TableDialogValues>) => {
    const next = { ...v, ...patch };
    set({ ...patch, borderWidth: widthAfterToggle(v.borderWidth, next.borderExternal || next.borderHeader, defaultLine) });
  };
  const toggleSeparators = (patch: Partial<TableDialogValues>) => {
    const next = { ...v, ...patch };
    set({ ...patch, separatorsWidth: widthAfterToggle(v.separatorsWidth, next.rowSeparators || next.colSeparators, defaultLine) });
  };

  const save = async () => {
    setBusy(true);
    try {
      if (await host.submit(v)) host.close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={host.close}>
      <div className="dialog" data-testid="table-properties" style={{ width: 760, maxHeight: "88vh", overflowY: "auto" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Table Properties</span>
        </div>
        <div className="dialog-body">
          <div style={{ overflow: "auto", maxHeight: "40vh", border: "1px solid var(--border, #555)", marginBottom: 8 }}>
            <table style={{ borderCollapse: "collapse", width: "100%", tableLayout: "fixed" }} data-testid="table-grid">
              <tbody>
                {v.grid.map((line, row) => (
                  <tr key={row}>
                    {line.map((text, gcol) => (
                      <td key={gcol} style={{ border: "1px solid var(--border, #555)", padding: 0, verticalAlign: "top" }}>
                        <textarea
                          data-testid={`table-cell-${row}-${gcol}`}
                          value={text}
                          disabled={covered(row, gcol)}
                          rows={Math.max(1, text.split("\n").length)}
                          style={{ width: "100%", boxSizing: "border-box", border: "none", resize: "none", background: covered(row, gcol) ? "rgba(128,128,128,0.25)" : "transparent", color: "inherit", font: "inherit" }}
                          onChange={(e) => {
                            const grid = v.grid.map((l) => l.slice());
                            grid[row]![gcol] = e.target.value;
                            set({ grid });
                          }}
                        />
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div style={{ fontSize: 11, opacity: 0.75, marginBottom: 8 }}>
            {rows} x {cols} cells.{" "}
            {sch
              ? 'A sheet counts rows and columns from 0: ${ROW}, ${COL} and ${ADDR} stand for the cell\'s place, ${CELL("A0")} for another cell\'s text; a cell another one covers has no text.'
              : "${ROW}, ${COL}, ${ADDR} and ${LAYER} stand for the cell's place and layer; a cell another one covers has no text."}
            {table && !sch && isBackSide(table.layer) ? " The table is on a back layer: its columns are shown from the other side." : ""}
          </div>
          {!sch && (
            <div className="kv-grid" style={GRID}>
              <span>Layer</span>
              <select value={v.layer} data-testid="table-layer" onChange={(e) => set({ layer: e.target.value })}>
                {!layers.includes(v.layer) && <option value={v.layer}>{v.layer}</option>}
                {layers.map((l) => (
                  <option key={l} value={l}>
                    {host.layerName(l)}
                  </option>
                ))}
              </select>
              {host.locked !== null && (
                <>
                  <span />
                  <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <input type="checkbox" data-testid="table-locked" checked={v.locked} onChange={(e) => set({ locked: e.target.checked })} /> Locked
                  </label>
                </>
              )}
            </div>
          )}
          <div style={{ fontWeight: 600, fontSize: 11, marginTop: 10, marginBottom: 4 }}>Border</div>
          <div className="kv-grid" style={GRID}>
            <span />
            <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input type="checkbox" data-testid="table-border-external" checked={v.borderExternal} onChange={(e) => toggleBorder({ borderExternal: e.target.checked })} /> Outline the table
            </label>
            <span />
            <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input type="checkbox" data-testid="table-border-header" checked={v.borderHeader} onChange={(e) => toggleBorder({ borderHeader: e.target.checked })} /> Line under the header row
            </label>
            <span>Border width ({unit})</span>
            <LengthField value={borderOn ? v.borderWidth : null} unit={unit} disabled={!borderOn} testId="table-border-width" onChange={(um) => set({ borderWidth: um ?? 0 })} />
            <span>Border style</span>
            <StyleSelect value={v.borderStyle} disabled={!borderOn} testId="table-border-style" onChange={(s) => set({ borderStyle: s })} />
          </div>
          <div style={{ fontWeight: 600, fontSize: 11, marginTop: 10, marginBottom: 4 }}>Separators</div>
          <div className="kv-grid" style={GRID}>
            <span />
            <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input type="checkbox" data-testid="table-separators-rows" checked={v.rowSeparators} onChange={(e) => toggleSeparators({ rowSeparators: e.target.checked })} /> Row separators
            </label>
            <span />
            <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input type="checkbox" data-testid="table-separators-cols" checked={v.colSeparators} onChange={(e) => toggleSeparators({ colSeparators: e.target.checked })} /> Cell separators (columns)
            </label>
            <span>Separators width ({unit})</span>
            <LengthField value={separatorsOn ? v.separatorsWidth : null} unit={unit} disabled={!separatorsOn} testId="table-separators-width" onChange={(um) => set({ separatorsWidth: um ?? 0 })} />
            <span>Separators style</span>
            <StyleSelect value={v.separatorsStyle} disabled={!separatorsOn} testId="table-separators-style" onChange={(s) => set({ separatorsStyle: s })} />
          </div>
        </div>
        <div className="dialog-footer">
          <button data-testid="table-cancel" onClick={host.close}>
            Cancel
          </button>
          <button className="primary" data-testid="table-ok" onClick={save} disabled={busy}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

const isBackSide = (layer: string): boolean => layer.startsWith("B.");

// ------------------------------------------------------------------------------------------------------------------------ Table Cell Properties

export function TableCellPropertiesDialog() {
  const dialogs = useTableDialogs();
  const open = dialogs.cells;
  const state = useStudioState();
  const api = useStudioApi();
  const unit: LengthUnit = state.units;
  const sch = open?.scope === "sch";
  const host = sch ? { drawings: { tables: state.schematic?.tables ?? [] } } : state.board;
  const table = open ? tableById(host, open.tableId) : undefined;
  const cells = open && table ? open.cells.flatMap(([r, c]) => (table.cells[r * table.cols + c] ? [table.cells[r * table.cols + c]!] : [])) : [];
  const [v, setV] = useState<CellDialogValues | null>(null);
  const [busy, setBusy] = useState(false);
  const key = open ? `${open.tableId}:${open.cells.map((c) => c.join(",")).join(";")}` : "";

  useEffect(() => {
    if (!open || cells.length === 0) {
      setV(null);
      return;
    }
    setV(cellDialogValues(cells));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  if (!open || !table || !v || cells.length === 0) return null;
  const set = (patch: Partial<CellDialogValues>) => setV((cur) => (cur ? { ...cur, ...patch } : cur));
  /** `onTextSize` / `onBoldToggle`: while the width is automatic the field shows the pen the size and boldness give. */
  const withAutoPen = (next: CellDialogValues): CellDialogValues => {
    if (!next.autoThickness) return next;
    const pen = autoPenFor(next.width, next.height, next.bold);
    return pen === null ? next : { ...next, thickness: pen };
  };
  const update = (patch: Partial<CellDialogValues>) => setV((cur) => (cur ? withAutoPen({ ...cur, ...patch }) : cur));

  const apply = async (): Promise<boolean> => {
    setBusy(true);
    try {
      // a sheet's dialog has the text colour and the background fill too (eeschema `DIALOG_TABLECELL_PROPERTIES`); the board's has neither
      const verb: TableCmdWire = { verb: "edit_cells", id: table.id, cells: open.cells, edit: sch ? { ...cellEditOf(v), ...cellColorEditOf(v) } : cellEditOf(v) };
      return await api.cmd(sch ? { op: "sch_table", ...verb } : { op: "table", ...verb });
    } finally {
      setBusy(false);
    }
  };
  const ok = async () => {
    if (await apply()) closeTableCellDialog();
  };
  const editTable = async () => {
    if (await apply()) {
      closeTableCellDialog();
      openTableDialog(table.id, sch ? "sch" : "pcb");
    }
  };
  const cellNames = open.cells.map(([r, c]) => (sch ? schCellAddress(r, c) : `${String.fromCharCode(65 + (c % 26))}${r + 1}`)).join(", ");

  return (
    <div className="dialog-backdrop" onClick={closeTableCellDialog}>
      <div className="dialog" data-testid="table-cell-properties" style={{ width: 520, maxHeight: "88vh", overflowY: "auto" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Table Cell Properties ({cellNames})</span>
        </div>
        <div className="dialog-body">
          <textarea
            data-testid="cell-text"
            value={v.text ?? ""}
            placeholder={v.text === null ? "<...>" : ""}
            rows={4}
            style={{ width: "100%", boxSizing: "border-box", marginBottom: 8, font: "inherit" }}
            onChange={(e) => set({ text: e.target.value })}
          />
          <div className="kv-grid" style={GRID}>
            <span>Font</span>
            <input type="text" data-testid="cell-face" value={v.face ?? ""} placeholder={v.face === null ? "<...>" : "Default Font"} onChange={(e) => set({ face: e.target.value })} />
            {sch ? (
              // `DIALOG_TABLECELL_PROPERTIES` of a sheet has one size for the glyphs, and no thickness
              <>
                <span>Text size ({unit})</span>
                <LengthField value={v.height} unit={unit} testId="cell-size" onChange={(um) => update({ width: um, height: um })} />
              </>
            ) : (
              <>
                <span>Text width ({unit})</span>
                <LengthField value={v.width} unit={unit} testId="cell-width" onChange={(um) => update({ width: um })} />
                <span>Text height ({unit})</span>
                <LengthField value={v.height} unit={unit} testId="cell-height" onChange={(um) => update({ height: um })} />
                <span>Text thickness ({unit})</span>
                <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                  <LengthField value={v.thickness} unit={unit} disabled={v.autoThickness} testId="cell-thickness" onChange={(um) => set({ thickness: um })} />
                  <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 4, whiteSpace: "nowrap" }}>
                    <input type="checkbox" data-testid="cell-auto-thickness" checked={v.autoThickness} onChange={(e) => update({ autoThickness: e.target.checked })} /> Auto
                  </label>
                </span>
              </>
            )}
            <span>Style</span>
            <span style={{ display: "flex", gap: 12 }}>
              <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 4 }}>
                <input type="checkbox" data-testid="cell-bold" checked={v.bold} onChange={(e) => update({ bold: e.target.checked })} /> Bold
              </label>
              <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 4 }}>
                <input type="checkbox" data-testid="cell-italic" checked={v.italic} onChange={(e) => set({ italic: e.target.checked })} /> Italic
              </label>
            </span>
            <span>Horizontal alignment</span>
            <span style={{ display: "flex", gap: 10 }}>
              {(["left", "center", "right"] as const).map((a) => (
                <label key={a} className="filter-row" style={{ display: "flex", alignItems: "center", gap: 4 }}>
                  <input type="radio" data-testid={`cell-halign-${a}`} checked={v.hAlign === a} onChange={() => set({ hAlign: a })} /> {a[0]!.toUpperCase() + a.slice(1)}
                </label>
              ))}
            </span>
            <span>Vertical alignment</span>
            <span style={{ display: "flex", gap: 10 }}>
              {(["top", "center", "bottom"] as const).map((a) => (
                <label key={a} className="filter-row" style={{ display: "flex", alignItems: "center", gap: 4 }}>
                  <input type="radio" data-testid={`cell-valign-${a}`} checked={v.vAlign === a} onChange={() => set({ vAlign: a })} /> {a[0]!.toUpperCase() + a.slice(1)}
                </label>
              ))}
            </span>
            {sch && (
              <>
                <span>Text color:</span>
                <CellColorField value={v.textColor} label="Text color" testId="cell-text-color" onChange={(c) => set({ textColor: c })} />
                <span>Background fill:</span>
                <CellColorField value={v.fillColor} label="Background fill" testId="cell-fill-color" onChange={(c) => set({ fillColor: c })} />
              </>
            )}
            {(["Left", "Top", "Right", "Bottom"] as const).map((side, i) => (
              <MarginRow key={side} label={`${side} margin (${unit})`} value={v.margins[i]!} unit={unit} testId={`cell-margin-${side.toLowerCase()}`} onChange={(um) => set({ margins: v.margins.map((m, j) => (j === i ? um : m)) as CellDialogValues["margins"] })} />
            ))}
          </div>
        </div>
        <div className="dialog-footer">
          <button data-testid="cell-edit-table" onClick={editTable} disabled={busy} style={{ marginRight: "auto" }}>
            Edit Table...
          </button>
          <button data-testid="cell-cancel" onClick={closeTableCellDialog}>
            Cancel
          </button>
          <button className="primary" data-testid="cell-ok" onClick={ok} disabled={busy}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * The colour of a sheet's cell dialog (`m_textColorBook` / `m_fillColorBook`): the swatch, its "Default" tick being `COLOR4D::UNSPECIFIED`; while the selected cells
 * disagree, the `-- mixed values --` / `Set Color...` popup, whose second choice shows the swatch (`onTextColorPopup`: `m_textColorBook->SetSelection( 1 )`).
 */
function CellColorField(props: { value: CellColor | null | "mixed"; label: string; testId: string; onChange: (c: CellColor | null) => void }) {
  if (props.value === "mixed") {
    return (
      <select data-testid={`${props.testId}-popup`} aria-label={props.label} value="mixed" onChange={(e) => e.target.value === "set" && props.onChange({ r: 0, g: 0, b: 0, a: 255 })}>
        <option value="mixed">-- mixed values --</option>
        <option value="set">Set Color...</option>
      </select>
    );
  }
  return (
    <span data-testid={props.testId}>
      <ColorSwatch label={props.label} color={props.value} onChange={(c) => props.onChange(c && c.a > 0 ? c : null)} />
    </span>
  );
}

function MarginRow(props: { label: string; value: number | null; unit: LengthUnit; testId: string; onChange: (um: number | null) => void }) {
  return (
    <>
      <span>{props.label}</span>
      <LengthField value={props.value} unit={props.unit} testId={props.testId} onChange={props.onChange} />
    </>
  );
}

/** Both dialogs, mounted once in the app. */
export function TableDialogs() {
  return (
    <>
      <TablePropertiesDialog />
      <TableCellPropertiesDialog />
    </>
  );
}

/** The cell a selection id names, for the actions that open a dialog. */
export function cellOf(board: Parameters<typeof cellById>[0], id: string): TableCellAddr | null {
  const f = cellById(board, id);
  return f ? [f.cell.row, f.cell.col] : null;
}
