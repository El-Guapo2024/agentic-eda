// dialog_gendrill.cpp ("Generate Drill Files", File > Fabrication Outputs > Drill Files..., and the Plot dialog's "Generate Drill Files..."),
// with the controls, labels and defaults of dialog_gendrill_base.cpp (KiCad 8303b2ad): "Output folder:", Format (Excellon with "Mirror Y
// axis", "Minimal header", "PTH and NPTH in single file", "Use alternate drill mode for oval holes"; Gerber X2 with "Generate tenting layers"),
// "Generate map:" and its format, the Options (Origin, Units, Zeros, Precision), the Messages, "Generate Report File...", Generate and Close.
// The logic is kicad-port/drillDialog.ts.
//
// Generate first stores the Origin and the output folder in the board's plot settings when they changed (`DIALOG_GENDRILL::updateConfig`:
// `SetPlotOptions` and `OnModify`), as `set_plot_settings`, no undo step (round 4 X4-3), then POSTs to /api/fab/drill
// (crates/cli/src/fab_api.rs), which runs `kicad-cli pcb export drill` with every control mapped onto its flag, into the output folder (shared
// with the Plot dialog; empty: export/kicad/drill/). The report is the writers' ("Created file '..'", then "Done."). "Generate Report File..."
// writes `<board>-drl.rpt` in the same folder (POST /api/fab/drill_report), KiCad's save dialog's default name.
import { useEffect, useState } from "react";
import { postFabDrill, postFabDrillReport } from "../api/client";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { DEFAULT_DRILL_FORM, drillEnables, drillFormAtOpening, drillPrecision, drillRequest, drillSettingsCommand, MAP_FORMATS, ZEROS, type DrillForm } from "../kicad-port/drillDialog";

/** The controls as the dialog last had them (`DIALOG_SHIM`'s saved control state), while the page lives. */
let kept: DrillForm = DEFAULT_DRILL_FORM;

export function GenerateDrillDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [form, setForm] = useState<DrillForm>(() => drillFormAtOpening(kept, state.board?.plot_params));
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);
  // `DIALOG_GENDRILL::TransferDataToWindow` runs at each opening: the Origin and the folder are the board's as it is then.
  useEffect(() => {
    if (!state.generateDrillDialogOpen) return;
    setForm(drillFormAtOpening(kept, state.board?.plot_params));
    setResult(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.generateDrillDialogOpen]);

  if (!state.generateDrillDialogOpen) return null;
  const set = (patch: Partial<DrillForm>) =>
    setForm((f) => {
      const next = { ...f, ...patch };
      kept = next;
      return next;
    });
  const close = () => dispatch({ type: "SET_GENERATE_DRILL_DIALOG_OPEN", open: false });
  const en = drillEnables(form);
  const gerberPrecision = Number(state.board?.plot_params?.options?.gerberprecision ?? 6);
  const precision = drillPrecision(form, gerberPrecision);

  /** `updateConfig` (on Generate and on Generate Report File...), then the run. */
  const run = async (report: boolean) => {
    setBusy(true);
    setResult(null);
    try {
      const cmd = drillSettingsCommand(form, state.board?.plot_params);
      if (cmd && !(await api.cmd(cmd))) return;
      const reply = report ? await postFabDrillReport(drillRequest(form)) : await postFabDrill(drillRequest(form));
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? "Done." : "Failed."), files: reply.files ?? [] });
    } finally {
      setBusy(false);
    }
  };

  const excellonStyle = { opacity: en.excellon ? 1 : 0.5 };
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Generate Drill Files</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <label style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 10 }}>
            Output folder:
            <input type="text" value={form.outputDirectory} placeholder="export/kicad/drill/" onChange={(e) => set({ outputDirectory: e.target.value })} style={{ flex: 1 }} data-testid="drill-output-dir" />
            <button disabled title="Not available: the studio runs in a browser: type the folder, relative to the board's folder">…</button>
          </label>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <div>
              <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 4 }}>Format</div>
              <label className="toggle" style={{ display: "block" }}>
                <input type="radio" name="drill-format" checked={form.format === "excellon"} onChange={() => set({ format: "excellon" })} />
                Excellon
              </label>
              <div style={{ paddingLeft: 20, ...excellonStyle }}>
                <label className="toggle" style={{ display: "block" }} title={"Not recommended.\nUsed mostly by users who make the boards themselves."}>
                  <input type="checkbox" disabled={!en.excellon} checked={form.mirrorY} onChange={(e) => set({ mirrorY: e.target.checked })} />
                  Mirror Y axis
                </label>
                <label className="toggle" style={{ display: "block" }} title={"Not recommended.\nOnly use it for board houses which do not accept fully featured headers."}>
                  <input type="checkbox" disabled={!en.excellon} checked={form.minimalHeader} onChange={(e) => set({ minimalHeader: e.target.checked })} />
                  Minimal header
                </label>
                <label className="toggle" style={{ display: "block" }} title={"Not recommended.\nOnly use for board houses which ask for merged PTH and NPTH into a single file."}>
                  <input type="checkbox" disabled={!en.excellon} checked={form.mergePthNpth} onChange={(e) => set({ mergePthNpth: e.target.checked })} />
                  PTH and NPTH in single file
                </label>
                <label className="toggle" style={{ display: "block" }}>
                  <input type="checkbox" disabled={!en.excellon} checked={form.altDrillMode} onChange={(e) => set({ altDrillMode: e.target.checked })} />
                  Use alternate drill mode for oval holes
                </label>
              </div>
              <label className="toggle" style={{ display: "block", marginTop: 4 }}>
                <input type="radio" name="drill-format" checked={form.format === "gerber"} onChange={() => set({ format: "gerber" })} />
                Gerber X2
              </label>
              <div style={{ paddingLeft: 20, opacity: en.tenting ? 1 : 0.5 }}>
                <label className="toggle" style={{ display: "block" }}>
                  <input type="checkbox" disabled={!en.tenting} checked={form.generateTenting} onChange={(e) => set({ generateTenting: e.target.checked })} />
                  Generate tenting layers
                </label>
              </div>
              <label style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 8 }}>
                <input type="checkbox" checked={form.generateMap} onChange={(e) => set({ generateMap: e.target.checked })} data-testid="drill-generate-map" />
                Generate map:
                <select value={form.mapFormat} onChange={(e) => set({ mapFormat: e.target.value as DrillForm["mapFormat"] })} data-testid="drill-map-format">
                  {MAP_FORMATS.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.label}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <div>
              <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 4 }}>Options</div>
              <div style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 6px", alignItems: "center" }}>
                {/* m_origin: "Absolute" or "Drill/place file origin" (pcbnew.EditorControl.drillOrigin). */}
                <span>Origin:</span>
                <select value={form.useAuxOrigin ? "aux" : "abs"} onChange={(e) => set({ useAuxOrigin: e.target.value === "aux" })} data-testid="drill-origin">
                  <option value="abs">Absolute</option>
                  <option value="aux">Drill/place file origin</option>
                </select>
                <span style={excellonStyle}>Units:</span>
                <select disabled={!en.excellon} value={form.unitsMm ? "mm" : "in"} onChange={(e) => set({ unitsMm: e.target.value === "mm" })} data-testid="drill-units">
                  <option value="mm">Millimeters</option>
                  <option value="in">Inches</option>
                </select>
                <span style={excellonStyle}>Zeros:</span>
                <select disabled={!en.excellon} value={form.zeros} onChange={(e) => set({ zeros: e.target.value as DrillForm["zeros"] })} data-testid="drill-zeros">
                  {ZEROS.map((z) => (
                    <option key={z.id} value={z.id}>
                      {z.label}
                    </option>
                  ))}
                </select>
                <span style={{ opacity: precision.enabled ? 1 : 0.5 }}>Precision:</span>
                <span style={{ opacity: precision.enabled ? 1 : 0.5 }} data-testid="drill-precision">
                  {precision.text}
                </span>
              </div>
            </div>
          </div>
          <div style={{ marginTop: 10 }}>
            <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 4 }}>Messages</div>
            {result ? (
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
            ) : (
              <div style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>Written by kicad-cli pcb export drill, KiCad's own drill and map writers.</div>
            )}
          </div>
        </div>
        <div className="dialog-footer" style={{ justifyContent: "space-between" }}>
          <button onClick={() => run(true)} disabled={busy}>
            Generate Report File...
          </button>
          <span style={{ display: "flex", gap: 8 }}>
            <button className="primary" onClick={() => run(false)} disabled={busy}>
              {busy ? "Generating…" : "Generate"}
            </button>
            <button onClick={close}>Close</button>
          </span>
        </div>
      </div>
    </div>
  );
}
