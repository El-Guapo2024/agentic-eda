// "Bulk Edit Symbol Fields..." (eeschema.EditorControl.editSymbolFields --
// eeschema/dialogs/dialog_symbol_fields_table.cpp, grid model in
// eeschema/fields_data_model.cpp): a table of every symbol x field.
//
// Edit tab: the grid, with
//   * "Group symbols" + per-column "Group By" (default: Value + Footprint,
//     the "Grouped By Value and Footprint" shape) -- grouped rows show a
//     collapsed reference text like "R1-R3, R5" (`SCH_REFERENCE_LIST::
//     Shorthand`) and a Qty, and expand into one child row per symbol;
//   * show/hide per column, click a header to sort (`m_sortColumn`);
//   * add / rename / remove user field columns;
//   * inline cell edits (not Reference, not the generated Qty): typing in a
//     group row sets the field on every symbol in it (`SetValue`), and a
//     group whose members disagree shows "-- mixed values --".
// Like the source's data store, edits are *staged* -- the grid regroups on
// them (the server overlays them on the schematic without saving, see
// sch_api.rs) -- and only "Apply" commits them, as ONE undoable
// `set_symbol_fields` verb (`ApplyData` + one SCH_COMMIT).
//
// Export tab: `BOM_FMT_PRESET` options (CSV / TSV / Semicolons presets,
// field/string/reference/range delimiters, keep tabs / line breaks), a live
// preview (`PreviewRefresh`) and "Export" writing the file
// (`OnExport`) -- relative to the board directory, see sch_api.rs.
//
// Not ported (PARITY-sch.md): attribute columns (DNP / exclude from BOM...
// -- not in the IR), variants, "Include excluded from BOM" / "Exclude DNP"
// filters, the scope selector (this table covers the current design's
// symbols), saved view presets, and the sidebar field-name templates.
import { useEffect, useRef, useState } from "react";
import { exportBom, fetchFieldsTable } from "../api/client";
import { postBomFile } from "../api/schControlClient";
import { useSchControlDispatch, useSchControlState } from "../state/schControlStore";
import type { BomFmt, FieldsTableReply, FieldsTableRow, FieldsTableSpec } from "../api/types";
import {
  bomFmtPresets,
  emptyChanges,
  isEditableColumn,
  isEmptyChanges,
  matchingBomPreset,
  specAddColumn,
  specRemoveColumn,
  specRenameColumn,
  stageAddField,
  stageEdit,
  stageRemoveField,
  stageRenameField,
  validNewFieldName,
  type StagedChanges,
} from "../kicad-port/fieldsTableStage";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

const MIXED = "-- mixed values --";
const BUILTIN = new Set(["Reference", "Value", "Footprint", "Datasheet"]);

function isGenerated(name: string): boolean {
  return name.startsWith("${") && name.endsWith("}");
}

export function SymbolFieldsTableDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.schDialog === "fields_table";

  const [tab, setTab] = useState<"edit" | "export">("edit");
  const [spec, setSpec] = useState<FieldsTableSpec | null>(null);
  const [changes, setChanges] = useState<StagedChanges>(emptyChanges());
  const [table, setTable] = useState<FieldsTableReply | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [newField, setNewField] = useState("");

  const [fmt, setFmt] = useState<BomFmt>(bomFmtPresets()[0] as BomFmt);
  const [outPath, setOutPath] = useState("");
  const [preview, setPreview] = useState("");
  const [exportMsg, setExportMsg] = useState<string | null>(null);

  const requestId = useRef(0);

  // A fresh open starts from the dialog's default view with nothing staged -- on the Export tab when Generate Bill of Materials opened it (`ShowExportTab`).
  const schControl = useSchControlState();
  const schControlDispatch = useSchControlDispatch();
  useEffect(() => {
    if (!open) return;
    setTab(schControl.fieldsTableOnExport ? "export" : "edit");
    if (schControl.fieldsTableOnExport) schControlDispatch({ type: "SET_FIELDS_TABLE_ON_EXPORT", on: false });
    setSpec(null);
    setChanges(emptyChanges());
    setTable(null);
    setError(null);
    setExpanded(new Set());
    setNewField("");
    setExportMsg(null);
  }, [open]);

  // Re-run the grid whenever the view, the staged changes or the schematic itself changed.
  useEffect(() => {
    if (!open) return;
    const id = ++requestId.current;
    void fetchFieldsTable(spec, isEmptyChanges(changes) ? null : changes).then((reply) => {
      if (id !== requestId.current) return;
      if (!reply.ok) {
        setError(reply.message ?? "Could not build the table.");
        return;
      }
      setError(null);
      setTable(reply);
      if (spec === null) setSpec(reply.spec);
    });
  }, [open, spec, changes, state.schematic]);

  // Export preview (`PreviewRefresh`) follows the same inputs while that tab is showing.
  useEffect(() => {
    if (!open || tab !== "export" || !spec) return;
    let cancelled = false;
    void exportBom(spec, fmt, isEmptyChanges(changes) ? null : changes).then((reply) => {
      if (cancelled) return;
      if (reply.ok) {
        setPreview(reply.text ?? "");
        setOutPath((p) => (p === "" ? (reply.default_path ?? "export/bom.csv") : p));
      } else {
        setPreview(reply.message ?? "");
      }
    });
    return () => {
      cancelled = true;
    };
  }, [open, tab, spec, fmt, changes, state.schematic]);

  if (!open) return null;

  const dirty = !isEmptyChanges(changes);
  const closeNow = () => dispatch({ type: "SET_SCH_DIALOG", dialog: null });
  const close = () => {
    // `OnClose`'s HandleUnsavedChanges: "Save changes?"
    if (dirty && !window.confirm("Discard the changes that have not been applied?")) return;
    closeNow();
  };

  const updateSpec = (patch: Partial<FieldsTableSpec>) => setSpec((s) => (s ? { ...s, ...patch } : s));
  const setColumn = (name: string, patch: { show?: boolean; group_by?: boolean }) => setSpec((s) => (s ? { ...s, columns: s.columns.map((c) => (c.name === name ? { ...c, ...patch } : c)) } : s));

  const apply = async (): Promise<boolean> => {
    if (!dirty) return true;
    const ok = await api.cmd({ op: "set_symbol_fields", edits: changes.edits, add_fields: changes.add_fields, rename_fields: changes.rename_fields, remove_fields: changes.remove_fields });
    if (ok) {
      setChanges(emptyChanges());
      dispatch({ type: "TOAST", message: "Symbol fields updated.", kind: "info" });
    }
    return ok;
  };

  const commitCell = (row: FieldsTableRow, field: string, value: string) => {
    let next = changes;
    for (const ref of row.refs) next = stageEdit(next, ref, field, value);
    setChanges(next);
  };

  const addField = () => {
    if (!spec) return;
    const problem = validNewFieldName(spec, newField);
    if (problem) {
      dispatch({ type: "TOAST", message: problem, kind: "error" });
      return;
    }
    const name = newField.trim();
    setChanges(stageAddField(changes, name));
    setSpec(specAddColumn(spec, name));
    setNewField("");
  };

  const renameField = (name: string) => {
    if (!spec) return;
    const to = window.prompt("New field name", name);
    if (to === null || to.trim() === name) return;
    const problem = validNewFieldName(spec, to);
    if (problem) {
      dispatch({ type: "TOAST", message: problem, kind: "error" });
      return;
    }
    setChanges(stageRenameField(changes, name, to.trim()));
    setSpec(specRenameColumn(spec, name, to.trim()));
  };

  const removeField = (name: string) => {
    if (!spec) return;
    setChanges(stageRemoveField(changes, name));
    setSpec(specRemoveColumn(spec, name));
  };

  const toggleSort = (name: string) => {
    if (!spec) return;
    if (spec.sort_field === name) updateSpec({ sort_asc: !spec.sort_asc });
    else updateSpec({ sort_field: name, sort_asc: true });
  };

  const toggleExpanded = (key: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const columns = spec?.columns ?? [];
  const shown = columns.map((c, i) => ({ c, i })).filter(({ c }) => c.show);

  const renderRow = (row: FieldsTableRow, child: boolean, rowKey: string) => (
    <tr key={rowKey} style={{ background: child ? "var(--chrome-bg)" : undefined }}>
      <td style={{ width: 22, textAlign: "center", padding: "2px 4px" }}>
        {row.flag === "group" && (
          <button title={expanded.has(rowKey) ? "Collapse" : "Expand"} style={{ padding: "0 4px", fontSize: 10 }} onClick={() => toggleExpanded(rowKey)}>
            {expanded.has(rowKey) ? "v" : ">"}
          </button>
        )}
      </td>
      {shown.map(({ c, i }) => {
        const text = row.cells[i] ?? "";
        const mixed = row.mixed[i] ?? false;
        if (!isEditableColumn(c.name)) {
          return (
            <td key={c.name} style={{ padding: "2px 8px", color: child ? "var(--chrome-text-dim)" : undefined, paddingLeft: child && c.name === "Reference" ? 22 : 8 }}>
              {text}
            </td>
          );
        }
        return (
          <td key={c.name} style={{ padding: "1px 4px" }}>
            <input
              // A fresh uncontrolled input whenever the row/cell value changes (regroup, apply), so the committed text always shows.
              key={`${row.refs.join(",")}|${c.name}|${mixed ? MIXED : text}`}
              defaultValue={mixed ? "" : text}
              placeholder={mixed ? MIXED : ""}
              style={{ width: "100%", boxSizing: "border-box", fontSize: 11 }}
              onBlur={(e) => {
                const v = e.currentTarget.value;
                if (mixed ? v !== "" : v !== text) commitCell(row, c.name, v);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
            />
          </td>
        );
      })}
    </tr>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div
        className="dialog"
        style={{ width: 980, maxWidth: "96vw", maxHeight: "88vh" }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
        }}
      >
        <div className="dialog-header">
          <span>Symbol Fields Table</span>
          <span style={{ display: "flex", gap: 6 }}>
            <button className={tab === "edit" ? "primary" : undefined} onClick={() => setTab("edit")}>
              Edit Fields
            </button>
            <button className={tab === "export" ? "primary" : undefined} onClick={() => setTab("export")}>
              Export
            </button>
          </span>
        </div>

        <div className="dialog-body" style={{ paddingTop: 10 }}>
          {error && <div className="problem-row">{error}</div>}

          {tab === "edit" && spec && (
            <div style={{ display: "flex", gap: 14, alignItems: "flex-start" }}>
              <div style={{ width: 230, flexShrink: 0, fontSize: 11 }}>
                <div style={{ fontWeight: 600, marginBottom: 4 }}>Fields</div>
                <table style={{ width: "100%", borderCollapse: "collapse" }}>
                  <thead>
                    <tr style={{ color: "var(--chrome-text-dim)" }}>
                      <th style={{ textAlign: "left", fontWeight: 500 }}>Name</th>
                      <th title="Show column">Show</th>
                      <th title="Group symbols that share this value">Group</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {columns.map((c) => (
                      <tr key={c.name}>
                        <td style={{ padding: "2px 0" }}>{c.label || c.name}</td>
                        <td style={{ textAlign: "center" }}>
                          <input type="checkbox" checked={c.show} aria-label={`Show ${c.name}`} onChange={(e) => setColumn(c.name, { show: e.target.checked })} />
                        </td>
                        <td style={{ textAlign: "center" }}>
                          <input type="checkbox" checked={c.group_by} aria-label={`Group by ${c.name}`} onChange={(e) => setColumn(c.name, { group_by: e.target.checked })} />
                        </td>
                        <td style={{ whiteSpace: "nowrap" }}>
                          {!BUILTIN.has(c.name) && !isGenerated(c.name) && (
                            <>
                              <button style={{ fontSize: 10, padding: "0 4px" }} title="Rename field" onClick={() => renameField(c.name)}>
                                Ren
                              </button>
                              <button style={{ fontSize: 10, padding: "0 4px", marginLeft: 2 }} title="Remove field from every symbol" onClick={() => removeField(c.name)}>
                                Del
                              </button>
                            </>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <div style={{ display: "flex", gap: 4, marginTop: 8 }}>
                  <input
                    placeholder="New field name"
                    value={newField}
                    style={{ flex: 1, minWidth: 0, fontSize: 11 }}
                    onChange={(e) => setNewField(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") addField();
                    }}
                  />
                  <button onClick={addField}>Add Field</button>
                </div>
              </div>

              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 14, marginBottom: 8, fontSize: 11 }}>
                  <label className="toggle">
                    <input type="checkbox" checked={spec.group_symbols} onChange={(e) => updateSpec({ group_symbols: e.target.checked })} />
                    Group symbols
                  </label>
                  <label style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    Filter
                    <input value={spec.filter} placeholder="Reference…" style={{ fontSize: 11 }} onChange={(e) => updateSpec({ filter: e.target.value })} />
                  </label>
                  <span style={{ color: "var(--chrome-text-dim)" }}>{table ? `${table.rows.length} row${table.rows.length === 1 ? "" : "s"}` : ""}</span>
                </div>
                <div style={{ overflow: "auto", maxHeight: "52vh", border: "1px solid var(--chrome-border)", borderRadius: 6 }}>
                  <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
                    <thead>
                      <tr style={{ position: "sticky", top: 0, background: "var(--chrome-bg-raised)" }}>
                        <th />
                        {shown.map(({ c }) => (
                          <th key={c.name} style={{ textAlign: "left", padding: "4px 8px", cursor: "pointer", whiteSpace: "nowrap" }} title="Sort by this column" onClick={() => toggleSort(c.name)}>
                            {c.label || c.name}
                            {spec.sort_field === c.name ? (spec.sort_asc ? " ▲" : " ▼") : ""}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {table?.rows.map((row) => {
                        const key = row.refs[0] ?? String(row.item_number);
                        return [renderRow(row, false, key), ...(row.flag === "group" && expanded.has(key) ? row.children.map((ch) => renderRow(ch, true, `${key}/${ch.refs[0] ?? ""}`)) : [])];
                      })}
                    </tbody>
                  </table>
                  {table && table.rows.length === 0 && <div className="panel-empty">No symbols match.</div>}
                  {!table && <div className="panel-empty">Loading…</div>}
                </div>
                <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 0 }}>
                  Edits are staged and regroup the table; "Apply" saves them all as one undoable step. A group row edits every symbol in it.
                </p>
              </div>
            </div>
          )}

          {tab === "export" && spec && (
            <div style={{ display: "flex", gap: 14, alignItems: "flex-start" }}>
              <div style={{ width: 280, flexShrink: 0 }}>
                <div className="kv-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
                  <span>Format preset</span>
                  <select
                    value={matchingBomPreset(fmt)?.name ?? "custom"}
                    onChange={(e) => {
                      const p = bomFmtPresets().find((x) => x.name === e.target.value);
                      if (p) setFmt(p);
                    }}
                  >
                    {bomFmtPresets().map((p) => (
                      <option key={p.name} value={p.name}>
                        {p.name}
                      </option>
                    ))}
                    {matchingBomPreset(fmt) === null && <option value="custom">Custom</option>}
                  </select>
                  <span>Field delimiter</span>
                  <input value={fmt.field_delimiter === "\t" ? "\\t" : fmt.field_delimiter} onChange={(e) => setFmt({ ...fmt, field_delimiter: e.target.value === "\\t" ? "\t" : e.target.value })} />
                  <span>String delimiter</span>
                  <input value={fmt.string_delimiter} onChange={(e) => setFmt({ ...fmt, string_delimiter: e.target.value })} />
                  <span>Reference delimiter</span>
                  <input value={fmt.ref_delimiter} onChange={(e) => setFmt({ ...fmt, ref_delimiter: e.target.value })} />
                  <span>Ref range delimiter</span>
                  <input value={fmt.ref_range_delimiter} placeholder="(none: list every reference)" onChange={(e) => setFmt({ ...fmt, ref_range_delimiter: e.target.value })} />
                </div>
                <label className="toggle" style={{ display: "block", marginTop: 8, fontSize: 11 }}>
                  <input type="checkbox" checked={fmt.keep_tabs} onChange={(e) => setFmt({ ...fmt, keep_tabs: e.target.checked })} />
                  Keep tabs
                </label>
                <label className="toggle" style={{ display: "block", fontSize: 11 }}>
                  <input type="checkbox" checked={fmt.keep_line_breaks} onChange={(e) => setFmt({ ...fmt, keep_line_breaks: e.target.checked })} />
                  Keep line breaks
                </label>
                <div className="kv-grid" style={{ gridTemplateColumns: "120px 1fr", marginTop: 10 }}>
                  <span>Output file</span>
                  <input value={outPath} placeholder="export/bom.csv" title="A file inside this board's export/ folder; kicad-cli writes it from the saved design" onChange={(e) => setOutPath(e.target.value)} />
                </div>
                {exportMsg && <div style={{ marginTop: 8, fontSize: 11, color: "var(--chrome-text-dim)" }}>{exportMsg}</div>}
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Preview</div>
                <textarea readOnly value={preview} spellCheck={false} style={{ width: "100%", boxSizing: "border-box", height: "46vh", fontFamily: "monospace", fontSize: 11, whiteSpace: "pre" }} />
              </div>
            </div>
          )}
        </div>

        <div className="dialog-footer">
          {tab === "export" ? (
            <button
              className="primary"
              disabled={!spec || outPath.trim() === ""}
              onClick={async () => {
                if (!spec) return;
                // The file is kicad-cli's (`kicad-cli sch export bom`, run on the saved design): edits that have not been applied would not be in it.
                if (dirty) {
                  if (!window.confirm("Changes have not yet been applied. Apply them and export?")) return;
                  if (!(await apply())) return;
                }
                setExportMsg("Exporting with kicad-cli...");
                try {
                  const reply = await postBomFile({ spec, fmt, path: outPath.trim() });
                  setExportMsg(reply.ok ? `Wrote BOM output to '${reply.files?.[0] ?? outPath}'${reply.engine ? ` (${reply.engine})` : ""}` : (reply.message ?? "Export failed."));
                } catch (e) {
                  setExportMsg(`Export failed: ${e instanceof Error ? e.message : String(e)}`);
                }
              }}
            >
              Export
            </button>
          ) : (
            <>
              <button disabled={!dirty} onClick={() => setChanges(emptyChanges())} title="Discard the staged edits">
                Revert
              </button>
              <button className="primary" disabled={!dirty} onClick={() => void apply()}>
                Apply
              </button>
            </>
          )}
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
