// KiCad 10's Drill Chart Properties and Drill Groups dialogs (KiCad 25ec2335; kicad-port/drillChartDialog.ts holds what they show and send).
//
// Drill Chart Properties: `DIALOG_GENERATED_TABLE_PROPERTIES` with the drill chart's `PANEL_DRILL_CHART_OPTIONS` (pcbnew/dialogs/dialog_generated_table_properties.cpp,
// panel_drill_chart_options.cpp and their _base), opened by the chart's properties (`PCB_EDIT_FRAME::ShowPropertiesDialog`'s `PCB_DRILL_CHART_T`: "Not the generic table dialog,
// which would offer copper layers and direct editing of cells that the next rebuild discards") -- a double click on its heading or totals row, E, the context menu's
// Properties. Three pages: General (Layer, a documentation layer; Units; Decimal places; Locked), Columns (the column grid -- Show, Column, Heading, Align, Min Width -- with
// Move Up, Move Down and Reset to Default, the Include filter, Show totals row, Group holes by, Import Template..., Export Template...) and Appearance (External border, Header
// separator, Row separators, Column separators, Border width, Separator width). OK is "Edit Drill Chart": one `edit_drill_chart` (and the lock), one undo step.
//
// Drill Groups: `DIALOG_DRILL_GROUPS` (pcbnew/dialogs/dialog_drill_groups.cpp), Tools > Drill Groups... (`PCB_CONTROL::ShowDrillGroups`), modeless. The board's hole groups
// (`GET /api/drill_groups`), each with its mark, the profile's name and Freeze assignments; the selected group's Mark, Shape, Letter and Description, committed when another
// row is selected and on Close (`commitDetail`), Flash on Canvas (`FocusOnItems`: the group's holes brightened, the view brought to the first). It reloads as the board changes.
import { useEffect, useRef, useState } from "react";
import type { BoardDrillChart } from "../api/pcbItemTypes";
import type { BoardTable } from "../api/types";
import { pickTextFile, saveTextFile } from "../api/libraryClient";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { isDocumentationLayer } from "../kicad-port/drillChart";
import {
  ALIGN_CHOICES,
  columnLabel,
  columnRows,
  defaultColumns,
  drillChartEditCmd,
  drillChartForm,
  groupCells,
  GROUP_KEY_ROWS,
  markDetail,
  markEditCmd,
  MARK_MODE_CHOICES,
  moveColumnRow,
  onBorderChecked,
  readTemplate,
  shownColumns,
  templateJson,
  UNITS_CHOICES,
  type ColumnAlign,
  type DrillChartForm,
  type DrillGroupRow,
  type MarkDetail,
  type MarkMode,
} from "../kicad-port/drillChartDialog";
import { focusView } from "../kicad-port/pcbFind";
import { itemBounds } from "../kicad-port/pcbItems";
import { lineThicknessNm } from "../kicad-port/layerDefaults";
import { closeDrillChartDialog, closeDrillGroupsDialog, useKicad10Dialogs } from "../state/kicad10Dialogs";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { nmFrom, nmTo, type LengthUnit } from "../state/units";

const fmtLen = (nm: number, units: LengthUnit): string => String(Number(nmTo(nm, units).toFixed(units === "mm" ? 4 : 2)));

type Page = "general" | "columns" | "appearance";
const PAGES: [Page, string][] = [
  ["general", "General"],
  ["columns", "Columns"],
  ["appearance", "Appearance"],
];

function Tabs<T extends string>({ pages, page, onPage, testid }: { pages: readonly [T, string][]; page: T; onPage: (p: T) => void; testid: string }) {
  return (
    <div role="tablist" style={{ display: "flex", gap: 2, borderBottom: "1px solid var(--chrome-border)", marginBottom: 8 }}>
      {pages.map(([p, label]) => (
        <button
          key={p}
          role="tab"
          aria-selected={page === p}
          onClick={() => onPage(p)}
          data-testid={`${testid}-${p}`}
          style={{ borderBottomLeftRadius: 0, borderBottomRightRadius: 0, fontWeight: page === p ? 700 : 400, background: page === p ? "var(--chrome-active)" : undefined }}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

/** The dialog's state: the form, the Min Width and stroke width fields as typed, the column grid's cursor row. */
interface Open {
  form: DrillChartForm;
  widths: string[];
  border: string;
  separators: string;
  cursor: number;
}

export function DrillChartPropertiesDialog() {
  const state = useStudioState();
  const api = useStudioApi();
  const { drillChart: req } = useKicad10Dialogs();
  const units = state.units;
  const board = state.board;
  const table: BoardTable | undefined = req ? board?.drawings?.tables?.find((t) => t.id === req.id) : undefined;
  const chart: BoardDrillChart | undefined = req ? board?.drawings?.drill_charts?.find((c) => c.table_id === req.id) : undefined;
  const [open, setOpen] = useState<Open | null>(null);
  const [page, setPage] = useState<Page>("general");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!req || !table || !chart) {
      setOpen(null);
      return;
    }
    const form = drillChartForm(chart, table, (board?.locked ?? []).includes(table.id), board?.drawings?.drill_profile?.group_by ?? ["size", "slot", "plating", "span", "operation", "post_machining"]);
    setOpen({
      form,
      widths: form.rows.map((r) => (r.column.width_nm > 0 ? fmtLen(r.column.width_nm, units) : "")),
      border: form.borderWidthNm >= 0 ? fmtLen(form.borderWidthNm, units) : "",
      separators: form.separatorsWidthNm >= 0 ? fmtLen(form.separatorsWidthNm, units) : "",
      cursor: 0,
    });
    // "After DIALOG_SHIM has restored the last page ...": the General page.
    setPage("general");
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req || !table || !chart || !open || !board) return null;
  const close = () => closeDrillChartDialog();
  const f = open.form;
  const setForm = (patch: Partial<DrillChartForm>) => setOpen({ ...open, form: { ...f, ...patch } });
  const layerSetup = board.board_rules?.layer_setup;
  const layers = boardLayerList(board.layers ?? [], layerSetup).filter(isDocumentationLayer);
  const defaultWidth = () => lineThicknessNm(board.board_rules?.text_graphics, f.layer);
  /** `harvestColumnGrid`: the Min Width fields as lengths ("Empty rather than zero, since zero means the column sizes itself to its text"). */
  const harvested = (o: Open): DrillChartForm => ({
    ...o.form,
    rows: o.form.rows.map((r, i) => ({ ...r, column: { ...r.column, width_nm: o.widths[i]?.trim() ? Math.round(nmFrom(Number(o.widths[i]), units)) || 0 : 0 } })),
    borderWidthNm: o.border.trim() === "" ? -1 : Math.round(nmFrom(Number(o.border), units)),
    separatorsWidthNm: o.separators.trim() === "" ? -1 : Math.round(nmFrom(Number(o.separators), units)),
  });
  const setRows = (rows: DrillChartForm["rows"], cursor = open.cursor) =>
    setOpen({ ...open, form: { ...f, rows }, widths: rows.map((r) => (r.column.width_nm > 0 ? fmtLen(r.column.width_nm, units) : "")), cursor });
  const setRow = (i: number, patch: Partial<DrillChartForm["rows"][number]["column"]> & { shown?: boolean }) => {
    const { shown, ...col } = patch;
    const rows = f.rows.map((r, j) => (j === i ? { shown: shown ?? r.shown, column: { ...r.column, ...col } } : r));
    setOpen({ ...open, form: { ...f, rows }, cursor: i });
  };
  const move = (delta: -1 | 1) => {
    const h = harvested(open);
    const moved = moveColumnRow(h.rows, open.cursor, delta);
    if (moved) setRows(moved.rows, moved.row);
  };
  /** `onBorderChecked` after one of the four boxes changed. */
  const check = (patch: Partial<DrillChartForm>) => {
    const h = { ...harvested(open), ...patch };
    const next = onBorderChecked(h, defaultWidth());
    setOpen({
      ...open,
      form: { ...f, ...patch },
      border: next.borderWidthNm >= 0 && open.border.trim() === "" ? fmtLen(next.borderWidthNm, units) : open.border,
      separators: next.separatorsWidthNm >= 0 && open.separators.trim() === "" ? fmtLen(next.separatorsWidthNm, units) : open.separators,
    });
  };
  const importTemplate = async () => {
    const file = await pickTextFile(".json").catch(() => null);
    if (!file) return;
    const t = readTemplate(file.text);
    if ("error" in t) return setError(t.error);
    // `m_SetFormat( format )`: the columns, units and precision; then the template's totals row.
    const rows = columnRows(t.columns);
    setOpen({ ...open, form: { ...f, rows, units: t.units, precision: t.precision, showTotals: t.showTotals }, widths: rows.map((r) => (r.column.width_nm > 0 ? fmtLen(r.column.width_nm, units) : "")), cursor: 0 });
    setError(null);
  };
  const exportTemplate = () => {
    const h = harvested(open);
    // "The file names the template, so whoever opens the json can tell which one it is"
    saveTextFile(templateJson({ name: "drill_chart", version: 1, units: h.units, precision: h.precision, showTotals: h.showTotals, columns: shownColumns(h.rows) }), "drill_chart.json", "application/json");
  };
  const ok = async () => {
    const out = drillChartEditCmd(table.id, harvested(open), (board.locked ?? []).includes(table.id));
    if ("error" in out) return setError(out.error);
    if (await api.cmdBatch(out.cmds)) close();
    else setError("The board refused the drill chart.");
  };

  const box = (label: string, checked: boolean, onChange: (v: boolean) => void, testid: string) => (
    <label style={{ display: "block" }}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} data-testid={testid} /> {label}
    </label>
  );
  const borderOn = f.externalBorder || f.headerSeparator;
  const separatorsOn = f.rowSeparators || f.columnSeparators;
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 640 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Drill Chart Properties" data-testid="drill-chart-properties">
        <div className="dialog-header">
          <span>Drill Chart Properties</span>
        </div>
        <div className="dialog-body">
          <Tabs pages={PAGES} page={page} onPage={setPage} testid="dcp-page" />
          {page === "general" && (
            <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
              <span>Layer:</span>
              <select value={f.layer} onChange={(e) => setForm({ layer: e.target.value })} data-testid="dcp-layer">
                {layers.map((l) => (
                  <option key={l} value={l}>
                    {layerDisplayName(layerSetup, l)}
                  </option>
                ))}
              </select>
              <span>Units:</span>
              <select value={f.units} onChange={(e) => setForm({ units: e.target.value as DrillChartForm["units"] })} data-testid="dcp-units">
                {UNITS_CHOICES.map(([u, label]) => (
                  <option key={u} value={u}>
                    {label}
                  </option>
                ))}
              </select>
              <span>Decimal places:</span>
              <input type="number" min={0} max={6} step={1} value={f.precision} onChange={(e) => setForm({ precision: Math.min(6, Math.max(0, Math.trunc(Number(e.target.value) || 0))) })} style={{ width: 60 }} data-testid="dcp-precision" />
              <span />
              {box("Locked", f.locked, (v) => setForm({ locked: v }), "dcp-locked")}
            </div>
          )}
          {page === "columns" && (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <div style={{ display: "flex", gap: 8 }}>
                <div style={{ flex: 1, maxHeight: 240, overflowY: "auto", border: "1px solid var(--chrome-border)" }}>
                  <table className="grid-table" style={{ width: "100%", borderCollapse: "collapse" }} data-testid="dcp-columns">
                    <thead>
                      <tr>
                        <th>Show</th>
                        <th>Column</th>
                        <th>Heading</th>
                        <th>Align</th>
                        <th>Min Width</th>
                      </tr>
                    </thead>
                    <tbody>
                      {f.rows.map((r, i) => (
                        <tr key={r.column.id} onClick={() => setOpen({ ...open, cursor: i })} style={{ background: i === open.cursor ? "var(--chrome-active)" : undefined }} data-testid={`dcp-col-${r.column.id}`}>
                          <td>
                            <input type="checkbox" checked={r.shown} onChange={(e) => setRow(i, { shown: e.target.checked })} data-testid={`dcp-show-${r.column.id}`} />
                          </td>
                          <td>{columnLabel(r.column.id)}</td>
                          <td>
                            <input value={r.column.heading} onChange={(e) => setRow(i, { heading: e.target.value })} style={{ width: "100%" }} data-testid={`dcp-heading-${r.column.id}`} />
                          </td>
                          <td>
                            <select value={r.column.align} onChange={(e) => setRow(i, { align: e.target.value as ColumnAlign })} data-testid={`dcp-align-${r.column.id}`}>
                              {ALIGN_CHOICES.map(([a, label]) => (
                                <option key={a} value={a}>
                                  {label}
                                </option>
                              ))}
                            </select>
                          </td>
                          <td>
                            <input
                              value={open.widths[i] ?? ""}
                              onChange={(e) => setOpen({ ...open, widths: open.widths.map((w, j) => (j === i ? e.target.value : w)), cursor: i })}
                              style={{ width: 70 }}
                              data-testid={`dcp-width-${r.column.id}`}
                            />{" "}
                            {units}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  <button onClick={() => move(-1)} data-testid="dcp-up">
                    Move Up
                  </button>
                  <button onClick={() => move(1)} data-testid="dcp-down">
                    Move Down
                  </button>
                  <button onClick={() => setRows(columnRows(defaultColumns()), 0)} data-testid="dcp-reset">
                    Reset to Default
                  </button>
                </div>
              </div>
              <div style={{ display: "flex", gap: 12 }}>
                <fieldset style={{ flex: 1 }}>
                  <legend>Include</legend>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 4 }}>
                    {box("Plated holes", f.filter.plated, (v) => setForm({ filter: { ...f.filter, plated: v } }), "dcp-plated")}
                    {box("Non-plated holes", f.filter.non_plated, (v) => setForm({ filter: { ...f.filter, non_plated: v } }), "dcp-npth")}
                    {box("Vias", f.filter.vias, (v) => setForm({ filter: { ...f.filter, vias: v } }), "dcp-vias")}
                    {box("Slots", f.filter.slots, (v) => setForm({ filter: { ...f.filter, slots: v } }), "dcp-slots")}
                    {box("Backdrills", f.filter.backdrills, (v) => setForm({ filter: { ...f.filter, backdrills: v } }), "dcp-backdrills")}
                    {box("Castellated", f.filter.castellated, (v) => setForm({ filter: { ...f.filter, castellated: v } }), "dcp-castellated")}
                  </div>
                  <div style={{ marginTop: 6 }}>{box("Show totals row", f.showTotals, (v) => setForm({ showTotals: v }), "dcp-totals")}</div>
                </fieldset>
                <fieldset style={{ flex: 1 }} title="Grouping is shared by every drill chart and drill map on the board">
                  <legend>Group holes by</legend>
                  {GROUP_KEY_ROWS.map(([k, label]) =>
                    box(label, f.groupBy.includes(k), (v) => setForm({ groupBy: v ? [...f.groupBy, k] : f.groupBy.filter((x) => x !== k) }), `dcp-group-${k}`)
                  )}
                </fieldset>
              </div>
              <div style={{ display: "flex", gap: 6 }}>
                <button onClick={() => void importTemplate()} data-testid="dcp-import">
                  Import Template...
                </button>
                <button onClick={exportTemplate} data-testid="dcp-export">
                  Export Template...
                </button>
              </div>
            </div>
          )}
          {page === "appearance" && (
            <div>
              {box("External border", f.externalBorder, (v) => check({ externalBorder: v }), "dcp-external")}
              {box("Header separator", f.headerSeparator, (v) => check({ headerSeparator: v }), "dcp-header")}
              {box("Row separators", f.rowSeparators, (v) => check({ rowSeparators: v }), "dcp-rows")}
              {box("Column separators", f.columnSeparators, (v) => check({ columnSeparators: v }), "dcp-cols")}
              <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", marginTop: 8 }}>
                <span>Border width:</span>
                <span>
                  <input value={open.border} disabled={!borderOn} onChange={(e) => setOpen({ ...open, border: e.target.value })} style={{ width: 90 }} data-testid="dcp-border-width" /> {units}
                </span>
                <span>Separator width:</span>
                <span>
                  <input value={open.separators} disabled={!separatorsOn} onChange={(e) => setOpen({ ...open, separators: e.target.value })} style={{ width: 90 }} data-testid="dcp-separator-width" /> {units}
                </span>
              </div>
            </div>
          )}
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="dcp-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="dcp-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} data-testid="dcp-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

/** `GET /api/drill_groups`. */
interface GroupsReply {
  ok: boolean;
  message?: string;
  profile_name?: string;
  freeze_assignments?: boolean;
  groups?: DrillGroupRow[];
}

export function DrillGroupsDialog() {
  const state = useStudioState();
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const { drillGroups: shown } = useKicad10Dialogs();
  const [reply, setReply] = useState<GroupsReply | null>(null);
  const [row, setRow] = useState(-1);
  const [detail, setDetail] = useState<MarkDetail | null>(null);
  const keyRef = useRef<string | null>(null);
  const board = state.board;

  // `Reload` (and `scheduleReload` on every board change): the groups again, the row of the same group kept selected.
  useEffect(() => {
    if (!shown) return;
    let live = true;
    fetch("/api/drill_groups", { cache: "no-store" })
      .then((r) => r.json() as Promise<GroupsReply>)
      .then((r) => {
        if (!live) return;
        setReply(r);
        const groups = r.groups ?? [];
        const at = keyRef.current ? groups.findIndex((g) => g.key === keyRef.current) : -1;
        const i = at >= 0 ? at : groups.length > 0 ? 0 : -1;
        setRow(i);
        keyRef.current = i >= 0 ? groups[i]!.key : null;
        setDetail(i >= 0 ? markDetail(groups[i]!) : null);
      })
      .catch(() => live && setReply({ ok: false, message: "The board could not be read." }));
    return () => {
      live = false;
    };
  }, [shown, board]);

  if (!shown) return null;
  const groups = reply?.groups ?? [];
  /** `commitDetail( m_detailRow )`: the pane's mark as the group's, when it is not what the group shows already. */
  const commit = async () => {
    const g = row >= 0 ? groups[row] : undefined;
    if (!g || !detail) return;
    const cmd = markEditCmd(g.key, detail, markDetail(g));
    if (cmd) await api.cmdBatch([cmd]);
  };
  const select = async (i: number) => {
    if (i === row) return;
    await commit();
    setRow(i);
    keyRef.current = groups[i]?.key ?? null;
    setDetail(groups[i] ? markDetail(groups[i]!) : null);
  };
  const close = async () => {
    await commit();
    closeDrillGroupsDialog();
  };
  const freeze = (v: boolean) => void api.cmdBatch([{ op: "pcb_item", verb: "set_drill_profile", profile: { freeze_assignments: v } }]);
  // `onFlash`: "Brightens rather than selects, so the canvas selection the user already had survives".
  const flash = async () => {
    const g = row >= 0 ? groups[row] : undefined;
    if (!g) return;
    await commit();
    dispatch({ type: "SET_HOT", refs: g.members });
    const b = api.getState().board;
    const first = b ? g.members.map((id) => itemBounds(b, id)).find((x) => x) : null;
    const r = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
    if (first && r && r.width >= 50 && r.height >= 50) {
      const cx = (first[0] + first[2]) / 2;
      const cy = (first[1] + first[3]) / 2;
      dispatch({ type: "SET_VIEW", view: focusView(api.getState().view, r.width, r.height, [cx, cy, cx, cy]) });
    }
  };
  const valid = row >= 0 && !!detail;
  const setD = (patch: Partial<MarkDetail>) => detail && setDetail({ ...detail, ...patch });
  return (
    <div
      className="dialog"
      role="dialog"
      aria-label="Drill Groups"
      data-testid="drill-groups"
      style={{ position: "fixed", top: 90, right: 24, width: "min(600px, calc(100vw - 24px))", zIndex: 1900 }}
    >
      <div className="dialog-header">
        <span>Drill Groups</span>
      </div>
      <div className="dialog-body" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div style={{ display: "flex", gap: 12, alignItems: "center" }}>
          <span>Profile:</span>
          <span data-testid="dg-profile">{reply?.profile_name ? reply.profile_name : "Default"}</span>
          <label style={{ marginLeft: "auto" }}>
            <input type="checkbox" checked={reply?.freeze_assignments ?? true} onChange={(e) => freeze(e.target.checked)} data-testid="dg-freeze" /> Freeze assignments
          </label>
        </div>
        {reply && !reply.ok && <p style={{ color: "var(--chrome-danger)" }}>{reply.message}</p>}
        <div style={{ maxHeight: 220, overflowY: "auto", border: "1px solid var(--chrome-border)" }}>
          <table className="grid-table" style={{ width: "100%", borderCollapse: "collapse" }} data-testid="dg-grid">
            <thead>
              <tr>
                <th>Symbol</th>
                <th>Size</th>
                <th>Plated</th>
                <th>From / To</th>
                <th>Ops</th>
                <th>Sites</th>
              </tr>
            </thead>
            <tbody>
              {groups.map((g, i) => (
                <tr key={g.key} onClick={() => void select(i)} style={{ background: i === row ? "var(--chrome-active)" : undefined, cursor: "default" }} data-testid={`dg-row-${i}`}>
                  {groupCells(g).map((c, j) => (
                    <td key={j}>{c}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <fieldset>
          <legend>Selected group</legend>
          <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr" }}>
            <span>Mark:</span>
            <select value={detail?.mode ?? "shape"} disabled={!valid} onChange={(e) => setD({ mode: e.target.value as MarkMode })} data-testid="dg-mark">
              {MARK_MODE_CHOICES.map(([m, label]) => (
                <option key={m} value={m}>
                  {label}
                </option>
              ))}
            </select>
            <span>Shape:</span>
            <input type="number" min={0} max={63} step={1} value={detail?.shape ?? 0} disabled={!valid} onChange={(e) => setD({ shape: Math.min(63, Math.max(0, Math.trunc(Number(e.target.value) || 0))) })} style={{ width: 70 }} data-testid="dg-shape" />
            <span>Letter:</span>
            <input value={detail?.letter ?? ""} disabled={!valid} onChange={(e) => setD({ letter: e.target.value })} style={{ width: 70 }} data-testid="dg-letter" />
            <span>Description:</span>
            <input value={detail?.description ?? ""} disabled={!valid} onChange={(e) => setD({ description: e.target.value })} data-testid="dg-description" />
          </div>
          <div style={{ marginTop: 6 }}>
            <button disabled={!valid} onClick={() => void flash()} data-testid="dg-flash">
              Flash on Canvas
            </button>
          </div>
        </fieldset>
      </div>
      <div className="dialog-footer">
        <button onClick={() => void close()} data-testid="dg-close">
          Close
        </button>
      </div>
    </div>
  );
}
