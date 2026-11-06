// Port of eeschema/dialogs/dialog_lib_edit_pin_table.cpp and `SYMBOL_EDITOR_EDIT_TOOL::PinTable` (8303b2ad): the summary above the grid ("Pin numbers:",
// "Pin count:", "Duplicate pins:", `updateSummary`), KiCad's fourteen columns (Count, Number, Name, Electrical Type, Graphic Style, Orientation, Number
// Text Size, Name Text Size, Length, X Position, Y Position, Visible, Unit, Body Style) of which `ShowHideColumns( "0 1 2 3 4 5 9 10" )` shows eight to begin
// with and the column header's menu shows or hides each (`GRID_TRICKS::onGridLabelRightClick`), a click on a header sorting by it (`OnColSort`; by Count,
// then Number, to begin with), "Group by name" and the refresh button (`OnRebuildRows`), the unit, body style and selection filters (`OnFilterCheckBox`,
// `OnFilterChoice`), and Export to a file or the clipboard (`OnExportButtonClick`, `PIN_TABLE_EXPORT`). The cells show KiCad's names (`PinTypeNames()`,
// `PinShapeNames()`, `PinOrientationNames()`, the unit's display name) and lengths in the frame's units; the grid's rules are kicad-port/pinTableGrid.ts.
//
// As KiCad's, the table edits copies of the pins (`TransferDataToWindow`): OK writes them to the symbol as one undo step ("Edit Pins",
// kicad-port/pinTable.ts `pinTableCommands`), Cancel or closing with changes asks "Save changes?" (`OnClose`'s `HandleUnsavedChanges`), and
// on a derived symbol OK is "Read Only". The rows act on their own pins only: Synchronized Pins Mode does not reach the table (`UpdatePin`
// never calls `SynchronizePins`). A length edit is `ChangeLength` (the body end stays), Y is KiCad's internal Y (the file's negated), and Add Pin is
// `OnAddRow` (the last pin's look, a pin step on).
//
// Not ported: "Group Selected Pins" (the grid has no block selection of rows), Import (From File... / From Clipboard), the "Highlight on Cross-probe"
// menu and its focus on the canvas, the arithmetic of a typed length (`NUMERIC_EVALUATOR`: a number with an optional unit is read), and the cells'
// icons; see PARITY-symedit.md.
import { useEffect, useMemo, useRef, useState } from "react";
import type { LibrarySymbolPin } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { useStudioState } from "../../state/store";
import { PIN_ORIENTATIONS } from "../../kicad-port/pinOrientation";
import { addedPin, bodyStyleCellEditable, bodyStyleChoices, pinTableCommands, unitCellEditable, unitChoices } from "../../kicad-port/pinTable";
import {
  COL,
  COL_COUNT,
  COLUMN_LABELS,
  csvText,
  DEFAULT_SHOWN_COLUMNS,
  exportTable,
  INDETERMINATE_STATE,
  nextSort,
  NO_FILTERS,
  ORIENTATION_NAMES,
  parseTypedLength,
  pinTableSummary,
  rebuildRows,
  renumberGroup,
  rowValue,
  sortRows,
  updatePin,
  type GridContext,
  type RowFilters,
  type TablePin,
} from "../../kicad-port/pinTableGrid";
import { PIN_SHAPE_OPTIONS, PIN_TYPE_OPTIONS } from "../../kicad-port/symPinAlternates";
import { stringFromValue } from "../../kicad-port/fpFields";
import { saveTextFile, writeClipboardText } from "../../api/libraryClient";
import { askChoice } from "../library/libraryDialogs";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { unitLetter } from "../../kicad-port/unitLetter";

/** A text cell: its text is taken as typed (`live`), or when it loses the focus or Enter is pressed (a length, and a grouped row's Number). */
function TextCell({ value, onCommit, width, label, live }: { value: string; onCommit: (v: string) => void; width: number; label: string; live: boolean }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <input
      value={text}
      aria-label={label}
      onChange={(e) => {
        setText(e.target.value);
        if (live) onCommit(e.target.value);
      }}
      onBlur={() => {
        if (!live && text !== value) onCommit(text);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !live && text !== value) onCommit(text);
      }}
      style={{ width }}
    />
  );
}

/** A choice cell: KiCad's names; a grouped row whose pins differ shows "-- mixed values --", which changes nothing (`SetValue` returns on it). */
function ChoiceCell({ value, options, onCommit, label, disabled }: { value: string; options: readonly { value: string; label: string }[]; onCommit: (v: string) => void; label: string; disabled?: boolean }) {
  const mixed = value === INDETERMINATE_STATE;
  return (
    <select value={mixed ? "" : value} aria-label={label} disabled={disabled} onChange={(e) => e.target.value !== "" && onCommit(e.target.value)}>
      {mixed && <option value="">{INDETERMINATE_STATE}</option>}
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

let keySeq = 0;
const newKey = () => `pt${++keySeq}`;

export function PinTableDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const units = useStudioState().units;
  const sym = state.symbol;
  /** `m_pins`: the copies the table edits; `null` until it opens. */
  const [pins, setPins] = useState<TablePin[] | null>(null);
  /** `m_rows`: each row's pins, by key. */
  const [rows, setRows] = useState<string[][]>([]);
  const [before, setBefore] = useState<LibrarySymbolPin[]>([]);
  const [edited, setEdited] = useState(false);
  const [busy, setBusy] = useState(false);
  const [group, setGroup] = useState(false);
  const [shown, setShown] = useState<Set<number>>(() => new Set(DEFAULT_SHOWN_COLUMNS));
  /** The column the rows are sorted by (`GetSortingColumn`); null is the default, Count ascending, with no header marked. */
  const [sort, setSort] = useState<{ col: number; ascending: boolean } | null>(null);
  const [unitFilter, setUnitFilter] = useState(-1);
  const [styleFilter, setStyleFilter] = useState(-1);
  const [selectionFilter, setSelectionFilter] = useState(false);
  const [exportShownOnly, setExportShownOnly] = useState(false);
  const [columnsMenu, setColumnsMenu] = useState<{ x: number; y: number } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const pinsRef = useRef<TablePin[]>([]);
  pinsRef.current = pins ?? [];

  const ctx: GridContext | null = useMemo(
    () =>
      sym
        ? {
            sym,
            // `StringFromValue( v, aIncludeUnits )`: the frame's units, with their name when asked
            formatLength: (mm, withUnits) => {
              const s = stringFromValue(Math.round(mm * 1e6), units);
              return withUnits ? s : s.replace(/\s+\S+$/, "");
            },
            parseLength: (text) => parseTypedLength(text, units),
          }
        : null,
    [sym, units]
  );

  const selectedPins = (): LibrarySymbolPin[] => {
    const st = api.getState();
    return (st.symbol?.pins ?? []).filter((p) => !!p.id && st.selection.has(p.id));
  };
  const filters = (): RowFilters => ({ unit: unitFilter, bodyStyle: styleFilter, selection: selectionFilter ? selectedPins() : null });

  /** `RebuildRows`: the rows from the pins, as the filters, the grouping and the sort say. */
  const rebuild = (list: TablePin[], f: RowFilters = filters(), grouped = group) => {
    if (!ctx) return;
    setRows(rebuildRows(list, grouped, f, sort?.col ?? COL.PIN_COUNT, sort?.ascending ?? true, ctx));
  };

  useEffect(() => {
    if (!state.pinTableOpen || !ctx) {
      setPins(null);
      return;
    }
    // `TransferDataToWindow`: `GetGraphicalPins( 0, 0 )` -- every pin of every unit and body style, copied
    const orig = [...(api.getState().symbol?.pins ?? [])];
    const list = orig.map((p) => ({ key: newKey(), pin: { ...p } }));
    setBefore(orig);
    setPins(list);
    setEdited(false);
    setGroup(false);
    setShown(new Set(DEFAULT_SHOWN_COLUMNS));
    setSort(null);
    setUnitFilter(-1);
    setStyleFilter(-1);
    setSelectionFilter(false);
    setNotice(null);
    setRows(rebuildRows(list, false, NO_FILTERS, COL.PIN_COUNT, true, ctx));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.pinTableOpen]);

  if (!state.pinTableOpen || !sym || !pins || !ctx) return null;
  /** `!IsSymbolEditable() || IsSymbolAlias()`: a derived symbol's pins are its parent's. */
  const readOnly = state.derivedOwn !== null;
  const close = () => dispatch({ type: "SET_PIN_TABLE_OPEN", open: false });
  const byKey = new Map(pins.map((tp) => [tp.key, tp]));
  const rowPins = (r: readonly string[]) => r.map((k) => byKey.get(k)).filter((tp): tp is TablePin => !!tp);

  /** `SetValue`: the value into each pin of the row; a grouped row's Number renumbers, adds and removes its pins. */
  const setValue = (rowIndex: number, col: number, value: string) => {
    if (value === INDETERMINATE_STATE) return;
    const row = rows[rowIndex];
    if (!row) return;
    const cur = pinsRef.current;
    if (col === COL.NUMBER && group) {
      const map = new Map(cur.map((tp) => [tp.key, tp]));
      const { row: next, added, removed } = renumberGroup(
        row.map((k) => map.get(k)).filter((tp): tp is TablePin => !!tp),
        value,
        newKey
      );
      const changed = new Map(next.map((tp) => [tp.key, tp]));
      const gone = new Set(removed);
      setPins([...cur.filter((tp) => !gone.has(tp.key)).map((tp) => changed.get(tp.key) ?? tp), ...added]);
      setRows((rs) => rs.map((r, i) => (i === rowIndex ? next.map((tp) => tp.key) : r)));
      setEdited(true);
      return;
    }
    const keys = new Set(row);
    setPins(cur.map((tp) => (keys.has(tp.key) ? { key: tp.key, pin: updatePin(tp.pin, value, col, ctx) } : tp)));
    setEdited(true);
  };

  /** `OnAddRow`: a new pin, the last pin's orientation, type, shape, unit and body style, a pin step on; its row goes at the end. */
  const addRow = () => {
    const cur = pinsRef.current;
    const tp = { key: newKey(), pin: addedPin(cur.map((p) => p.pin)) };
    setPins([...cur, tp]);
    setRows((rs) => [...rs, [tp.key]]);
    setEdited(true);
  };

  /** `OnDeleteRows`: the row and its pins go. */
  const deleteRow = (rowIndex: number) => {
    const gone = new Set(rows[rowIndex] ?? []);
    setPins(pinsRef.current.filter((tp) => !gone.has(tp.key)));
    setRows((rs) => rs.filter((_, i) => i !== rowIndex));
    setEdited(true);
  };

  /** `TransferDataFromWindow` then `commit.Push( _( "Edit Pins" ) )`. */
  const save = async (): Promise<boolean> => {
    if (readOnly || !state.libId) return false;
    const cmds = pinTableCommands(
      state.libId,
      before,
      pinsRef.current.map((tp) => tp.pin)
    );
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

  /** `OnColSort`. */
  const sortBy = (col: number) => {
    const next = nextSort(sort, col);
    setSort(next);
    setRows(sortRows(rows.map(rowPins), next.col, next.ascending, ctx).map((r) => r.map((tp) => tp.key)));
  };

  /** `OnExportButtonClick`: every pin, or the rows the table shows ("Export only shown pins"), to a `.csv` file or the clipboard. */
  const exportPins = async (toFile: boolean) => {
    const list = exportShownOnly ? rows.flatMap((r) => rowPins(r).map((tp) => tp.pin)) : pinsRef.current.map((tp) => tp.pin);
    const text = csvText(exportTable(list, ctx));
    if (toFile) {
      // `wxFileName fn( m_symbol->GetName() ); fn.SetExt( FILEEXT::CsvFileExtension )`
      const name = sym.lib_id.includes(":") ? sym.lib_id.slice(sym.lib_id.indexOf(":") + 1) : sym.lib_id;
      saveTextFile(text, `${name}.csv`, "text/csv");
    } else {
      const wrote = await writeClipboardText(text);
      setNotice(wrote ? null : "The browser would not put the table on the clipboard.");
    }
  };

  const unitOpts = unitChoices(sym);
  const styles = bodyStyleChoices(sym);
  const unitEditable = unitCellEditable(sym);
  const styleEditable = bodyStyleCellEditable(sym);
  const summary = pinTableSummary(pins);
  const columns = Array.from({ length: COL_COUNT }, (_, i) => i).filter((c) => shown.has(c));
  const typeOptions = PIN_TYPE_OPTIONS.map((o) => ({ value: o.label, label: o.label }));
  const shapeOptions = PIN_SHAPE_OPTIONS.map((o) => ({ value: o.label, label: o.label }));
  const orientationOptions = PIN_ORIENTATIONS.map((o) => ({ value: ORIENTATION_NAMES[o], label: ORIENTATION_NAMES[o] }));

  // `GRID_TRICKS::onGridLabelRightClick`: a check item per column (`AppendCheckItem`, `Check( id, IsColShown( i ) )`)
  const columnsMenuEntries: MenuEntry[] = COLUMN_LABELS.map((label, c) => ({
    label: `${shown.has(c) ? "\u2713" : "\u2003"} ${label}`,
    checked: shown.has(c),
    onSelect: () =>
      setShown((s) => {
        const n = new Set(s);
        if (n.has(c)) n.delete(c);
        else n.add(c);
        return n;
      }),
  }));

  const cell = (rowIndex: number, list: TablePin[], col: number) => {
    const value = rowValue(
      list.map((tp) => tp.pin),
      col,
      ctx
    );
    const label = COLUMN_LABELS[col]!;
    const commit = (v: string) => setValue(rowIndex, col, v);
    switch (col) {
      case COL.PIN_COUNT:
        return <span aria-label={label}>{value}</span>;
      case COL.NUMBER:
        return <TextCell value={value} label={label} width={group ? 120 : 50} live={!group} onCommit={commit} />;
      case COL.NAME:
        return <TextCell value={value} label={label} width={80} live onCommit={commit} />;
      case COL.TYPE:
        return <ChoiceCell value={value} options={typeOptions} label={label} onCommit={commit} />;
      case COL.SHAPE:
        return <ChoiceCell value={value} options={shapeOptions} label={label} onCommit={commit} />;
      case COL.ORIENTATION:
        return <ChoiceCell value={value} options={orientationOptions} label={label} onCommit={commit} />;
      case COL.VISIBLE:
        return (
          <input
            type="checkbox"
            aria-label={label}
            checked={value === "1"}
            ref={(el) => {
              if (el) el.indeterminate = value === INDETERMINATE_STATE;
            }}
            onChange={(e) => commit(e.target.checked ? "1" : "0")}
          />
        );
      case COL.UNIT: {
        // `GRID_CELL_COMBOBOX( unitNames )`: "ALL" and each unit's display name; a symbol of one unit takes nothing (`if( !parent->IsMultiUnit() ) break;`)
        const opts = (unitEditable ? unitOpts : unitOpts.slice(0, 1)).map((c) => ({ value: c.label, label: c.label }));
        return <ChoiceCell value={value} options={opts} label={label} disabled={!unitEditable} onCommit={commit} />;
      }
      case COL.BODY_STYLE: {
        const opts = styles.map((c) => ({ value: c.label, label: c.label }));
        return <ChoiceCell value={value} options={opts} label={label} disabled={!styleEditable} onCommit={commit} />;
      }
      default:
        // the sizes, the length and the position: the frame's units, as `StringFromValue( .., true )` writes them; taken on Enter or when the cell is left
        return <TextCell value={value} label={label} width={80} live={false} onCommit={commit} />;
    }
  };

  const setUnitFilterTo = (f: number) => {
    setUnitFilter(f);
    rebuild(pinsRef.current, { ...filters(), unit: f });
  };
  const setStyleFilterTo = (f: number) => {
    setStyleFilter(f);
    rebuild(pinsRef.current, { ...filters(), bodyStyle: f });
  };

  return (
    <div className="dialog-backdrop" onClick={() => void cancel()}>
      <div className="dialog" style={{ width: 1040, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Pin Table" data-testid="pin-table">
        <div className="dialog-header">
          <span>Pin Table</span>
          <span>{sym.lib_id}</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", gap: 12 }}>
          <div style={{ flex: 1, minWidth: 0 }}>
            {/* `updateSummary` */}
            <div style={{ display: "flex", gap: 16, fontSize: 12, marginBottom: 6 }} data-testid="pin-table-summary">
              <span>
                Pin numbers:{" "}
                <span data-testid="pin-table-numbers" title={summary.numbers}>
                  {summary.numbers}
                </span>
              </span>
              <span>
                Pin count: <span data-testid="pin-table-count">{summary.count}</span>
              </span>
              <span>
                Duplicate pins:{" "}
                <span data-testid="pin-table-duplicates" title={summary.duplicates}>
                  {summary.duplicates}
                </span>
              </span>
            </div>
            <div style={{ maxHeight: "62vh", overflow: "auto" }}>
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                <thead
                  data-testid="pin-table-header"
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setColumnsMenu({ x: e.clientX, y: e.clientY });
                  }}
                >
                  <tr>
                    {columns.map((c) => (
                      <th
                        key={c}
                        data-col={COLUMN_LABELS[c]}
                        onClick={() => sortBy(c)}
                        style={{ textAlign: "left", padding: "2px 6px", borderBottom: "1px solid var(--chrome-border, #333)", cursor: "pointer", whiteSpace: "nowrap" }}
                      >
                        {COLUMN_LABELS[c]}
                        {sort?.col === c ? (sort.ascending ? " ▲" : " ▼") : ""}
                      </th>
                    ))}
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r, i) => {
                    const list = rowPins(r);
                    return (
                      <tr key={r.join("|") || `row-${i}`} data-testid="pin-table-row">
                        {columns.map((c) => (
                          <td key={c} style={{ padding: "1px 4px" }}>
                            {cell(i, list, c)}
                          </td>
                        ))}
                        <td>
                          <button onClick={() => deleteRow(i)} disabled={readOnly}>
                            Delete
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              {rows.length === 0 && <p style={{ color: "var(--chrome-text-dim)" }}>No pins yet.</p>}
            </div>
            {notice && <p style={{ color: "var(--chrome-text-dim)", margin: "6px 0 0" }}>{notice}</p>}
          </div>
          {/* the right pane: grouping, the filters and Export */}
          <div style={{ width: 230, display: "flex", flexDirection: "column", gap: 6, fontSize: 12 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <label>
                <input
                  type="checkbox"
                  checked={group}
                  data-testid="pin-table-group"
                  onChange={(e) => {
                    setGroup(e.target.checked);
                    rebuild(pinsRef.current, filters(), e.target.checked);
                  }}
                />{" "}
                Group by name
              </label>
              <button title="Refresh" data-testid="pin-table-refresh" onClick={() => rebuild(pinsRef.current)}>
                ↻
              </button>
            </div>
            {/* `m_cbFilterByUnit` / `m_unitFilter`: "ALL" and the units' letters (`LetterSubReference`), off on a symbol of one unit */}
            <label style={{ opacity: unitEditable ? 1 : 0.5 }}>
              <input type="checkbox" disabled={!unitEditable} checked={unitFilter !== -1} onChange={(e) => setUnitFilterTo(e.target.checked ? 0 : -1)} /> Filter by unit:
            </label>
            <select disabled={!unitEditable} value={unitFilter} aria-label="Unit filter" onChange={(e) => setUnitFilterTo(Number(e.target.value))}>
              <option value={-1} hidden />
              {unitEditable && <option value={0}>ALL</option>}
              {unitEditable &&
                Array.from({ length: sym.unit_count }, (_, k) => (
                  <option key={k + 1} value={k + 1}>
                    {unitLetter(k + 1)}
                  </option>
                ))}
            </select>
            <label style={{ opacity: styleEditable ? 1 : 0.5 }}>
              <input type="checkbox" disabled={!styleEditable} checked={styleFilter !== -1} onChange={(e) => setStyleFilterTo(e.target.checked ? 0 : -1)} /> Filter by body style:
            </label>
            <select disabled={!styleEditable} value={styleFilter} aria-label="Body style filter" onChange={(e) => setStyleFilterTo(Number(e.target.value))}>
              <option value={-1} hidden />
              {styleEditable && <option value={0}>ALL</option>}
              {styleEditable && <option value={1}>Standard</option>}
              {styleEditable && <option value={2}>Alternate</option>}
            </select>
            <label>
              <input
                type="checkbox"
                checked={selectionFilter}
                onChange={(e) => {
                  setSelectionFilter(e.target.checked);
                  rebuild(pinsRef.current, { ...filters(), selection: e.target.checked ? selectedPins() : null });
                }}
              />{" "}
              Filter by selection
            </label>
            <fieldset style={{ marginTop: 8 }}>
              <legend>Export</legend>
              <label>
                <input type="radio" checked={!exportShownOnly} onChange={() => setExportShownOnly(false)} /> Export all pins
              </label>
              <br />
              <label>
                <input type="radio" checked={exportShownOnly} onChange={() => setExportShownOnly(true)} /> Export only shown pins
              </label>
              <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
                <button onClick={() => void exportPins(true)}>To File...</button>
                <button onClick={() => void exportPins(false)} data-testid="pin-table-export-clipboard">
                  To Clipboard
                </button>
              </div>
            </fieldset>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={addRow} disabled={readOnly} data-testid="pin-table-add">
            Add Pin
          </button>
          <button onClick={() => void cancel()}>Cancel</button>
          <button className="primary" disabled={readOnly || busy} onClick={() => void ok()} data-testid="pin-table-ok">
            {readOnly ? "Read Only" : "OK"}
          </button>
        </div>
        {columnsMenu && <ContextMenu x={columnsMenu.x} y={columnsMenu.y} entries={columnsMenuEntries} onClose={() => setColumnsMenu(null)} />}
      </div>
    </div>
  );
}
