// pcbnew.Control.showNetInspector ("Net Inspector"): `PCB_NET_INSPECTOR_PANEL` (pcbnew/widgets/pcb_net_inspector_panel.cpp).
//
// KiCad's columns, filter and report (kicad-port/netInspector.ts) over the numbers the backend measures as KiCad does (`GET /api/net_inspector`,
// crates/cli/src/net_inspector_api.rs: `calculateNets`, `LENGTH_DELAY_CALCULATION::CalculateLengthDetails`): Name, Netclass, Total Length,
// Via Count, Via Length, Track Length, Die Length, Pad Count and one length per copper layer; a click on a column heading sorts by it (again: the other way); the search box
// with Filter by Net Name / Filter by Netclass, Show Zero Pad Nets and Show Unconnected Nets; a click on a row highlights its net (again: clears
// it); Save Net Inspector Report... writes KiCad's CSV.
//
// Different from KiCad: a dialog over the board, not a docked panel; no grouping (Group by Netclass, custom groups), no time-domain columns, and
// none of the net edits of the panel's menu (Rename / Delete / Add Net).
import { useEffect, useMemo, useState } from "react";
import { saveTextFile } from "../api/libraryClient";
import { DEFAULT_NET_INSPECTOR_OPTIONS, netInspectorColumns, netInspectorCell, netInspectorReport, netInspectorRows, type NetInspectorColumn, type NetInspectorOptions, type NetInspectorRow } from "../kicad-port/netInspector";
import { useStudioDispatch, useStudioState } from "../state/store";

export function NetInspectorDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [rows, setRows] = useState<NetInspectorRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [opts, setOpts] = useState<NetInspectorOptions>(DEFAULT_NET_INSPECTOR_OPTIONS);
  const [sort, setSort] = useState<{ column: NetInspectorColumn; ascending: boolean }>({ column: "name", ascending: true });
  const open = state.netInspectorOpen;

  // The numbers again whenever the board changes while the panel is open (`OnBoardChanged` / `OnBoardItemsChanged`).
  useEffect(() => {
    if (!open) return;
    let live = true;
    fetch("/api/net_inspector")
      .then((r) => r.json())
      .then((j: { ok: boolean; nets?: NetInspectorRow[]; message?: string }) => {
        if (!live) return;
        if (j.ok) {
          setRows(j.nets ?? []);
          setError(null);
        } else setError(j.message ?? "The net inspector could not read the board.");
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [open, state.board]);

  const shown = useMemo(() => netInspectorRows(rows ?? [], opts, sort.column, sort.ascending), [rows, opts, sort]);
  const columns = useMemo(() => netInspectorColumns(state.board?.layers ?? []), [state.board?.layers]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_NET_INSPECTOR_OPEN", open: false });
  const units = state.units;
  const toggle = (key: keyof Omit<NetInspectorOptions, "filter">, label: string) => (
    <label style={{ display: "inline-flex", gap: 4, alignItems: "center", marginRight: 10 }}>
      <input type="checkbox" checked={opts[key]} onChange={(e) => setOpts({ ...opts, [key]: e.target.checked })} data-testid={`net-inspector-${key}`} />
      {label}
    </label>
  );
  const heading = (column: NetInspectorColumn, label: string) => (
    <span
      key={column}
      role="columnheader"
      data-testid={`net-inspector-col-${column}`}
      style={{ fontWeight: 600, cursor: "pointer", textAlign: column === "name" || column === "netclass" ? "left" : "right", whiteSpace: "nowrap" }}
      onClick={() => setSort(sort.column === column ? { column, ascending: !sort.ascending } : { column, ascending: true })}
    >
      {label}
      {sort.column === column ? (sort.ascending ? " ▲" : " ▼") : ""}
    </span>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 900, maxWidth: "95vw", maxHeight: "85vh" }} onClick={(e) => e.stopPropagation()} data-testid="net-inspector">
        <div className="dialog-header">
          <span>Net Inspector</span>
          <span>{shown.length} net(s)</span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 8, overflow: "auto" }}>
          <input autoFocus placeholder="Filter" value={opts.filter} onChange={(e) => setOpts({ ...opts, filter: e.target.value })} style={{ width: "100%", padding: "5px 8px", marginBottom: 6 }} data-testid="net-inspector-filter" />
          <div style={{ fontSize: 11, marginBottom: 8 }}>
            {toggle("filterByNetName", "Filter by Net Name")}
            {toggle("filterByNetclass", "Filter by Netclass")}
            {toggle("showZeroPadNets", "Show Zero Pad Nets")}
            {toggle("showUnconnectedNets", "Show Unconnected Nets")}
          </div>
          {error && <div className="panel-empty">{error}</div>}
          <div role="grid" className="kv-grid" style={{ gridTemplateColumns: `minmax(140px, 2fr) minmax(70px, 1fr) repeat(${columns.length - 2}, auto)`, columnGap: 12, rowGap: 3, fontSize: 12 }}>
            {columns.map((c) => heading(c.key, c.label))}
            {shown.map((r) => {
              const hot = state.netHighlight === r.name;
              return (
                <span
                  key={r.name}
                  role="row"
                  data-testid="net-inspector-row"
                  data-net={r.name}
                  style={{ display: "contents", cursor: "default" }}
                  onClick={() => dispatch({ type: "SET_NET_HIGHLIGHT", net: hot ? null : r.name })}
                >
                  {columns.map((c) => (
                    <span key={c.key} data-col={c.key} style={{ textAlign: c.key === "name" || c.key === "netclass" ? "left" : "right", fontWeight: hot ? 600 : 400, color: hot ? "var(--chrome-accent)" : c.key === "name" ? undefined : "var(--chrome-text-dim)", whiteSpace: "nowrap" }}>
                      {netInspectorCell(r, c.key, units)}
                    </span>
                  ))}
                </span>
              );
            })}
          </div>
          {rows && shown.length === 0 && <span className="panel-empty">No nets.</span>}
        </div>
        <div className="dialog-footer">
          <button onClick={() => saveTextFile(netInspectorReport(shown, units, columns), "net-inspector.csv", "text/csv")} data-testid="net-inspector-report">
            Save Net Inspector Report...
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
