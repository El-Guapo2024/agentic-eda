// File > Export > Netlist... -- eeschema's DIALOG_EXPORT_NETLIST (dialog_export_netlist.cpp and dialog_export_netlist_base.cpp, KiCad
// 8303b2ad): the notebook's pages KiCad, OrcadPCB2, Allegro, CadStar, PADS, SPICE (with its options) and SPICE Model, the messages,
// "Add Exporter...", "Remove Exporter", "Export Netlist" and Close; plus the generic XML, which kicad-cli has (`kicadxml`) and KiCad's dialog
// reaches through its exporters.
//
// POSTs to /api/sch/netlist (crates/cli/src/sch_output_api.rs), which runs `kicad-cli sch export netlist --format <format>` on the exported
// schematic, into export/kicad/sch-netlist/, named `<project>.<ext>` by the format (KiCad's save dialog starts with that name). Shown disabled
// with the reason: the SPICE page's options (kicad-cli's job writes the netlist with the simulation command only, which is the page with every
// box unchecked), "Run external simulator command:" and the custom exporters (the studio runs no external command).
import { useEffect, useState } from "react";
import { postSchNetlist } from "../api/client";
import { buildSchNetlistRequest, NETLIST_CLI_LIMITS, SCH_NETLIST_FORMATS, SPICE_OPTIONS, summarizeOutputs, type SchNetlistFormat } from "../kicad-port/schOutputs";
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
    setResult(null);
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_EXPORT_NETLIST_DIALOG_OPEN", open: false });
  const page = SCH_NETLIST_FORMATS.find((f) => f.id === format) ?? SCH_NETLIST_FORMATS[0];

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

  const spice = format === "spice" || format === "spicemodel";
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Export Netlist</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div role="tablist" style={{ display: "flex", flexWrap: "wrap", gap: 2, borderBottom: "1px solid var(--chrome-border)", marginBottom: 8 }}>
            {SCH_NETLIST_FORMATS.map((f) => (
              <button
                key={f.id}
                role="tab"
                aria-selected={format === f.id}
                onClick={() => setFormat(f.id)}
                style={{ borderBottomLeftRadius: 0, borderBottomRightRadius: 0, fontWeight: format === f.id ? 600 : 400, background: format === f.id ? "var(--chrome-bg-raised)" : undefined }}
                data-testid={`netlist-tab-${f.id}`}
              >
                {f.label}
              </button>
            ))}
          </div>
          <div style={{ marginBottom: 10 }}>
            <div style={{ marginBottom: 10 }}>
              {page?.text} ({page?.extension})
            </div>
            {spice && (
              <div style={{ opacity: 0.6 }} title={`Not available: ${NETLIST_CLI_LIMITS.spice}`}>
                {SPICE_OPTIONS.filter((o) => format === "spice" || o.models).map((o) => (
                  <label key={o.label} style={{ display: "block" }} title={`${o.tip} -- not available: ${NETLIST_CLI_LIMITS.spice}`}>
                    <input type="checkbox" disabled checked={false} readOnly /> {o.label}
                  </label>
                ))}
                {format === "spice" && (
                  <div style={{ marginTop: 6 }} title={`Not available: ${NETLIST_CLI_LIMITS.simulator}`}>
                    <label style={{ display: "block" }} title={'Enter the command line to run SPICE\nUsually \'<path to SPICE binary> "%I"\'\n%I will be replaced by the netlist filepath'}>
                      <input type="checkbox" disabled checked={false} readOnly /> Run external simulator command:
                    </label>
                    <input type="text" disabled value={'spice "%I"'} readOnly style={{ width: "100%" }} />
                  </div>
                )}
              </div>
            )}
          </div>
          {result && (
            <div className={result.ok ? "panel-empty" : "problem-row"} style={{ fontSize: 11 }}>
              <b style={{ whiteSpace: "pre-line" }}>{result.message}</b>
              {result.files.length > 0 && (
                <ul style={{ margin: "6px 0 0 16px", padding: 0 }}>
                  {result.files.map((f) => (
                    <li key={f}>{f}</li>
                  ))}
                </ul>
              )}
            </div>
          )}
          {!result && (
            <div style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>
              Output directory: <code>export/kicad/sch-netlist/</code> (inside this board's own directory). Written by <code>kicad-cli sch export netlist</code>, so the nets are numbered and ordered the way KiCad does it.
            </div>
          )}
        </div>
        <div className="dialog-footer" style={{ justifyContent: "space-between" }}>
          <span style={{ display: "flex", gap: 8 }}>
            <button disabled title={`Not available: ${NETLIST_CLI_LIMITS.exporters}`}>
              Add Exporter...
            </button>
            <button disabled title={`Not available: ${NETLIST_CLI_LIMITS.exporters}`}>
              Remove Exporter
            </button>
          </span>
          <span style={{ display: "flex", gap: 8 }}>
            <button className="primary" onClick={exportNetlist} disabled={busy}>
              {busy ? "Exporting…" : "Export Netlist"}
            </button>
            <button onClick={close}>Close</button>
          </span>
        </div>
      </div>
    </div>
  );
}
