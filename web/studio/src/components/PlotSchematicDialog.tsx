// File > Plot... on the Schematic tab -- eeschema's DIALOG_PLOT_SCHEMATIC
// (dialog_plot_schematic.cpp, SCH_PLOT_OPTS): output format, colour vs.
// black and white, "Plot drawing sheet", "Use background color", page size,
// and all pages vs. the current one. The real dialog also offers PostScript/
// DXF/PNG, a colour-theme chooser and PDF property popups -- not ported
// (see PARITY-sch.md section 7).
//
// POSTs to /api/sch/plot (crates/cli/src/sch_api.rs), which runs
// eda_kicad::plot_schematic -- the SCH_PLOTTER / SVG_PLOTTER / PDF_PLOTTER
// port, not a kicad-cli call -- and writes into this board's own export/
// folder (one .svg per sheet, or one multi-page .pdf).
import { useEffect, useState } from "react";
import { postSchPlot } from "../api/client";
import { buildSchPlotRequest, DEFAULT_SCH_PLOT_FORM, summarizeOutputs, type SchPlotForm } from "../kicad-port/schOutputs";
import { useStudioDispatch, useStudioState } from "../state/store";

export function PlotSchematicDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.schPlotDialogOpen;
  const currentSheetIds = (state.schematic?.sheet_path ?? []).map((s) => s.id);

  const [form, setForm] = useState<SchPlotForm>(DEFAULT_SCH_PLOT_FORM);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);

  useEffect(() => {
    if (!open) return;
    setForm({ ...DEFAULT_SCH_PLOT_FORM });
    setResult(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_SCH_PLOT_DIALOG_OPEN", open: false });
  const set = (patch: Partial<SchPlotForm>) => setForm((prev) => ({ ...prev, ...patch }));

  const plot = async () => {
    setBusy(true);
    setResult(null);
    try {
      const reply = await postSchPlot(buildSchPlotRequest({ ...form, currentSheetIds }));
      const files = reply.files ?? [];
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? summarizeOutputs(files) : "Plot failed."), files });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Plot Schematic</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <fieldset style={{ marginBottom: 10 }}>
            <legend>Output format</legend>
            <label style={{ display: "block" }}>
              <input type="radio" checked={form.format === "svg"} onChange={() => set({ format: "svg" })} /> SVG (one file per sheet)
            </label>
            <label style={{ display: "block" }}>
              <input type="radio" checked={form.format === "pdf"} onChange={() => set({ format: "pdf" })} /> PDF (one page per sheet)
            </label>
          </fieldset>
          <fieldset style={{ marginBottom: 10 }}>
            <legend>Pages</legend>
            <label style={{ display: "block" }}>
              <input type="radio" checked={form.scope === "all"} onChange={() => set({ scope: "all" })} /> Plot all pages (every sheet in the hierarchy)
            </label>
            <label style={{ display: "block" }}>
              <input type="radio" checked={form.scope === "current"} onChange={() => set({ scope: "current" })} /> Plot current page only
            </label>
          </fieldset>
          <fieldset style={{ marginBottom: 10 }}>
            <legend>Options</legend>
            <label style={{ display: "block" }}>
              <input type="radio" checked={form.color} onChange={() => set({ color: true })} /> Color
            </label>
            <label style={{ display: "block" }}>
              <input type="radio" checked={!form.color} onChange={() => set({ color: false })} /> Black and white
            </label>
            <label style={{ display: "block", marginTop: 6 }}>
              <input type="checkbox" checked={form.plotDrawingSheet} onChange={(e) => set({ plotDrawingSheet: e.target.checked })} /> Plot drawing sheet (frame and title block)
            </label>
            <label style={{ display: "block", opacity: form.color ? 1 : 0.5 }}>
              <input type="checkbox" disabled={!form.color} checked={form.useBackgroundColor} onChange={(e) => set({ useBackgroundColor: e.target.checked })} /> Use background color
            </label>
          </fieldset>
          <label style={{ display: "block", marginBottom: 10 }}>
            Page size:{" "}
            <select value={form.pageSize} onChange={(e) => set({ pageSize: e.target.value as SchPlotForm["pageSize"] })}>
              <option value="auto">Schematic size</option>
              <option value="a4">A4</option>
              <option value="a">A (US letter)</option>
            </select>
          </label>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginBottom: 10 }}>
            Output directory: <code>export/</code> (inside this board's own directory), named like KiCad's own plot files (<code>&lt;project&gt;[-&lt;sheet&gt;].svg</code> / <code>.pdf</code>).
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
          <button className="primary" onClick={plot} disabled={busy}>
            {busy ? "Plotting…" : "Plot"}
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
