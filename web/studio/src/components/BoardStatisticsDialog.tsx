// Port of pcbnew/dialogs/dialog_board_statistics{,_base}.cpp -- "Show
// Board Statistics" (pcbnew.InspectionTool.ShowBoardStatistics). Read-only:
// every number comes from `POST /api/board_stats`
// (crates/cli/src/board_stats.rs, a port of `ComputeBoardStatistics`), so
// there is no Cmd and nothing to undo.
//
// Same layout as source: a "General" page (Components / Pads / Vias / Board
// grids) and a "Drill Holes" page (sortable by any column header, toggling
// ascending/descending like `drillGridSort`), the three checkboxes that
// re-run the computation on every click (`checkboxClicked`), and
// "Generate Report File..." writing `FormatBoardStatisticsReport`'s text
// as `<board>_report.txt` (`saveReportClicked`). The cancel button reads
// "Close" -- "Nothing to cancel". Checkbox state persists for the session
// like source's `s_savedDialogState`.
import { useEffect, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { postBoardStats } from "../api/client";
import type { BoardStatsDrill, BoardStatsOptions, BoardStatsReply } from "../api/types";
import { messageTextFromValue } from "../kicad-port/messageText";

/** `s_savedDialogState` -- module-level, survives closing the dialog. */
let savedOptions: BoardStatsOptions = {
  exclude_footprints_without_pads: false,
  subtract_holes_from_board_area: false,
  subtract_holes_from_copper_areas: false,
};

type DrillCol = "qty" | "shape" | "x_um" | "y_um" | "plated" | "is_pad" | "start_layer" | "stop_layer";

const DRILL_COLS: { key: DrillCol; label: string }[] = [
  { key: "qty", label: "Count" },
  { key: "shape", label: "Shape" },
  { key: "x_um", label: "X Size" },
  { key: "y_um", label: "Y Size" },
  { key: "plated", label: "Plated" },
  { key: "is_pad", label: "Via/Pad" },
  { key: "start_layer", label: "Start Layer" },
  { key: "stop_layer", label: "Stop Layer" },
];

/** `DRILL_LINE_ITEM::COMPARE`'s `compareDrillParameters`. Layers compare
 * by their position in the copper stack (source compares `PCB_LAYER_ID`s). */
function compareDrills(a: BoardStatsDrill, b: BoardStatsDrill, col: DrillCol, ascending: boolean, layers: string[]): boolean {
  const key = (d: BoardStatsDrill): number => {
    switch (col) {
      case "qty":
        return d.qty;
      case "shape":
        return d.shape === "round" ? 0 : 1;
      case "x_um":
        return d.x_um;
      case "y_um":
        return d.y_um;
      case "plated":
        return d.plated ? 1 : 0;
      case "is_pad":
        return d.is_pad ? 1 : 0;
      case "start_layer":
        return d.start_layer === null ? -1 : layers.indexOf(d.start_layer);
      case "stop_layer":
        return d.stop_layer === null ? -1 : layers.indexOf(d.stop_layer);
    }
  };
  return ascending ? key(a) < key(b) : key(a) > key(b);
}

export function BoardStatisticsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.boardStatisticsDialogOpen;
  const units = state.units;
  const layers = state.board?.layers ?? ["F.Cu", "B.Cu"];

  const [opts, setOpts] = useState<BoardStatsOptions>(savedOptions);
  const [page, setPage] = useState<"general" | "drills">("general");
  const [stats, setStats] = useState<BoardStatsReply | null>(null);
  const [drills, setDrills] = useState<BoardStatsDrill[]>([]);
  const [sort, setSort] = useState<{ col: DrillCol; ascending: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    postBoardStats(opts).then((reply) => {
      if (cancelled) return;
      if (!reply.ok) {
        setError(reply.message ?? "could not compute board statistics");
        return;
      }
      setError(null);
      setStats(reply);
      setDrills(reply.drills ?? []);
      setSort(null);
    });
    return () => {
      cancelled = true;
    };
  }, [open, opts]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_BOARD_STATISTICS_DIALOG_OPEN", open: false });

  const toggle = (key: keyof BoardStatsOptions) => {
    const next = { ...opts, [key]: !opts[key] };
    savedOptions = next;
    setOpts(next);
  };

  const sortBy = (col: DrillCol) => {
    const ascending = !(sort?.col === col && sort.ascending);
    setSort({ col, ascending });
    setDrills((ds) => [...ds].sort((a, b) => (compareDrills(a, b, col, ascending, layers) ? -1 : compareDrills(b, a, col, ascending, layers) ? 1 : 0)));
  };

  const saveReport = async () => {
    const reply = await postBoardStats(opts, { units, date: new Date().toLocaleString() });
    if (!reply.ok || reply.report === undefined) {
      setError(reply.message ?? "could not generate the report");
      return;
    }
    const blob = new Blob([reply.report], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = reply.report_file_name ?? "board_report.txt";
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  };

  const dist = (um: number) => messageTextFromValue(um, units, true, "distance");
  const area = (um2: number) => messageTextFromValue(um2, units, true, "area");
  const b = stats?.board;

  const countTable = (rows: { title: string; qty: number }[], extra?: { title: string; qty: number }[]) => (
    <table className="setup-table">
      <tbody>
        {rows.map((r) => (
          <tr key={r.title}>
            <td>{r.title}</td>
            <td style={{ textAlign: "right" }}>{r.qty}</td>
          </tr>
        ))}
        <tr>
          <td>Total:</td>
          <td style={{ textAlign: "right" }}>{rows.reduce((s, r) => s + r.qty, 0)}</td>
        </tr>
        {(extra ?? []).map((r) => (
          <tr key={r.title}>
            <td>{r.title}</td>
            <td style={{ textAlign: "right" }}>{r.qty}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );

  const fps = stats?.footprints ?? [];
  const totalFront = fps.reduce((s, f) => s + f.front, 0);
  const totalBack = fps.reduce((s, f) => s + f.back, 0);

  const boardRows: [string, string][] = b
    ? [
        ["Dimensions:", b.has_outline ? `${messageTextFromValue(b.width_um, units, false)} x ${dist(b.height_um)}` : "unknown"],
        ["Area:", b.has_outline ? area(b.area_um2) : "unknown"],
        ["Front copper area:", area(b.front_copper_area_um2)],
        ["Back copper area:", area(b.back_copper_area_um2)],
        ["Min track clearance:", dist(b.min_clearance_um)],
        ["Min track width:", dist(b.min_track_width_um)],
        ["Min drill diameter:", dist(b.min_drill_um)],
        ["Board stackup thickness:", dist(b.thickness_um)],
        ["Front footprint area:", area(b.front_courtyard_area_um2)],
        ["Front footprint density:", b.has_outline ? `${b.front_density_pct.toFixed(2)} %` : ""],
        ["Back footprint area:", area(b.back_courtyard_area_um2)],
        ["Back footprint density:", b.has_outline ? `${b.back_density_pct.toFixed(2)} %` : ""],
      ]
    : [];

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 720, maxHeight: "85vh" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Board Statistics</span>
        </div>
        <div className="editor-tabs" style={{ padding: "4px 12px 0" }}>
          <button className={`editor-tab${page === "general" ? " active" : ""}`} onClick={() => setPage("general")}>
            General
          </button>
          <button className={`editor-tab${page === "drills" ? " active" : ""}`} onClick={() => setPage("drills")}>
            Drill Holes
          </button>
        </div>
        <div className="dialog-body" style={{ overflowY: "auto", fontSize: 12 }}>
          {error && <div style={{ color: "#ff6666", marginBottom: 6 }}>{error}</div>}
          {page === "general" && stats && (
            <div style={{ display: "grid", gridTemplateColumns: "auto auto", gap: 16, alignItems: "start" }}>
              <div>
                <div style={{ fontWeight: 600, marginBottom: 4 }}>Components</div>
                <table className="setup-table">
                  <thead>
                    <tr>
                      <th />
                      <th>Front Side</th>
                      <th>Back Side</th>
                      <th>Total</th>
                    </tr>
                  </thead>
                  <tbody>
                    {fps.map((f) => (
                      <tr key={f.title}>
                        <td>{f.title}</td>
                        <td style={{ textAlign: "right" }}>{f.front}</td>
                        <td style={{ textAlign: "right" }}>{f.back}</td>
                        <td style={{ textAlign: "right" }}>{f.front + f.back}</td>
                      </tr>
                    ))}
                    <tr>
                      <td>Total:</td>
                      <td style={{ textAlign: "right" }}>{totalFront}</td>
                      <td style={{ textAlign: "right" }}>{totalBack}</td>
                      <td style={{ textAlign: "right" }}>{totalFront + totalBack}</td>
                    </tr>
                  </tbody>
                </table>
                <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>Pads</div>
                {countTable(stats.pads ?? [], stats.pad_properties)}
                <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>Vias</div>
                {countTable(stats.vias ?? [])}
              </div>
              <div>
                <div style={{ fontWeight: 600, marginBottom: 4 }}>Board</div>
                <table className="setup-table">
                  <tbody>
                    {boardRows.map(([label, value]) => (
                      <tr key={label}>
                        <td>{label}</td>
                        <td style={{ textAlign: "right" }}>{value}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
          {page === "drills" && (
            <table className="setup-table">
              <thead>
                <tr>
                  {DRILL_COLS.map((c) => (
                    <th key={c.key} style={{ cursor: "pointer" }} onClick={() => sortBy(c.key)}>
                      {c.label}
                      {sort?.col === c.key ? (sort.ascending ? " ▲" : " ▼") : ""}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {drills.map((d, i) => (
                  <tr key={i}>
                    <td style={{ textAlign: "right" }}>{d.qty}</td>
                    <td>{d.shape === "round" ? "Round" : "Slot"}</td>
                    <td style={{ textAlign: "right" }}>{dist(d.x_um)}</td>
                    <td style={{ textAlign: "right" }}>{dist(d.y_um)}</td>
                    <td>{d.plated ? "PTH" : "NPTH"}</td>
                    <td>{d.is_pad ? "Pad" : "Via"}</td>
                    <td>{d.start_layer ?? "N/A"}</td>
                    <td>{d.stop_layer ?? "N/A"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div style={{ marginTop: 10 }}>
            <label className="filter-row" style={{ display: "block" }}>
              <input type="checkbox" checked={opts.exclude_footprints_without_pads} onChange={() => toggle("exclude_footprints_without_pads")} />
              Exclude footprints with no pads
            </label>
            <label className="filter-row" style={{ display: "block" }}>
              <input type="checkbox" checked={opts.subtract_holes_from_board_area} onChange={() => toggle("subtract_holes_from_board_area")} />
              Subtract holes from board area
            </label>
            <label className="filter-row" style={{ display: "block" }}>
              <input type="checkbox" checked={opts.subtract_holes_from_copper_areas} onChange={() => toggle("subtract_holes_from_copper_areas")} />
              Subtract holes from copper areas
            </label>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={saveReport} disabled={!stats}>
            Generate Report File...
          </button>
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
