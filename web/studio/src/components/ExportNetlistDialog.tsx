// File > Export > Netlist... -- eeschema's DIALOG_EXPORT_NETLIST
// (dialog_export_netlist.cpp): pick a netlist format and export. The real
// dialog also has Spice/Cadstar/OrcadPCB2/Allegro/PADS tabs and a plugin
// list -- only the two exporters ported here are offered (the KiCad `.net`
// s-expression, NETLIST_EXPORTER_KICAD, and the generic `.xml`,
// NETLIST_EXPORTER_XML); see PARITY-sch.md section 7.
//
// POSTs to /api/sch/netlist (crates/cli/src/sch_api.rs), which runs
// eda_kicad::export_netlist over the board's one netlist and writes into
// this board's own export/ folder.
import { useEffect, useState } from "react";
import { postSchNetlist } from "../api/client";
import { buildSchNetlistRequest, SCH_NETLIST_FORMATS, summarizeOutputs, type SchNetlistFormat } from "../kicad-port/schOutputs";
import { useStudioDispatch, useStudioState } from "../state/store";

export function ExportNetlistDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.exportNetlistDialogOpen;

  const [format, setFormat] = useState<SchNetlistFormat>("kicad");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);

  useEffect(() => {
    if (!open) return;
    setFormat("kicad");
    setResult(null);
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_EXPORT_NETLIST_DIALOG_OPEN", open: false });

  const exportNetlist = async () => {
    setBusy(true);
    setResult(null);
    try {
      const reply = await postSchNetlist(buildSchNetlistRequest(format));
      const files = reply.files ?? [];
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? summarizeOutputs(files) : "Export failed."), files });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Export Netlist</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <fieldset style={{ marginBottom: 10 }}>
            <legend>Netlist format</legend>
            {SCH_NETLIST_FORMATS.map((f) => (
              <label key={f.id} style={{ display: "block" }}>
                <input type="radio" checked={format === f.id} onChange={() => setFormat(f.id)} /> {f.label} ({f.extension})
              </label>
            ))}
          </fieldset>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginBottom: 10 }}>
            Output directory: <code>export/</code> (inside this board's own directory). Nets are numbered in KiCad's own order (sorted by name, nodes by reference then pin); power symbols and
            <code> #</code> references are left out.
          </div>
          {result && (
            <div className={result.ok ? "panel-empty" : "problem-row"} style={{ fontSize: 11 }}>
              <b>{result.message}</b>
              {result.files.length > 0 && (
                <ul style={{ margin: "6px 0 0 16px", padding: 0 }}>
                  {result.files.map((f) => (
                    <li key={f}>{f}</li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button className="primary" onClick={exportNetlist} disabled={busy}>
            {busy ? "Exporting…" : "Export Netlist"}
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
