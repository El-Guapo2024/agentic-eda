// File > Plot... on the Schematic tab -- eeschema's DIALOG_PLOT_SCHEMATIC (dialog_plot_schematic.cpp and dialog_plot_schematic_base.cpp,
// KiCad 8303b2ad, SCH_PLOT_OPTS), with its controls, labels and defaults: "Output directory:", "Design variant:", Output Format (Postscript,
// PDF, SVG, DXF, PNG), the Options (Page size, Plot drawing sheet, Output mode, Color theme, Plot background color, Minimum line width), the
// PDF, DXF and PNG options, "Open file after plot", the messages, "Plot All Pages", "Plot Current Page" and Close. The enable rules
// (`onPlotFormatSelection`, `onColorMode`) and the request are kicad-port/schOutputs.ts.
//
// POSTs to /api/sch/plot (crates/cli/src/sch_output_api.rs), which runs `kicad-cli sch export <format>` on the exported schematic with each
// option mapped onto its flag, into the dialog's output folder (relative to the board's folder; empty: export/kicad/sch-<format>/). The
// controls kicad-cli has no flag for (page size, minimum line width, DXF units) and the ones a browser cannot do (open the file, browse for a
// folder) are shown disabled with the reason. "Design variant:" is kicad-cli's `--variant` (KiCad 25ec2335 `SCH_PLOT_OPTS::m_variant`).
import { useEffect, useState } from "react";
import { postSchPlot } from "../api/client";
import { VariantChoice } from "./VariantChoice";
import { buildSchPlotRequest, DEFAULT_SCH_PLOT_FORM, SCH_CLI_LIMITS, SCH_COLOR_THEMES, SCH_PAGE_SIZES, SCH_PLOT_FORMATS, schPlotEnables, summarizeOutputs, type SchPlotForm, type SchPlotScope } from "../kicad-port/schOutputs";
import { useStudioDispatch, useStudioState } from "../state/store";

/** The choices of the last plot (`DIALOG_SHIM::LoadControlState` restores them), while the page lives. */
let kept: SchPlotForm = DEFAULT_SCH_PLOT_FORM;

const dim = { color: "var(--chrome-text-dim)", fontSize: 11 } as const;

export function PlotSchematicDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.schPlotDialogOpen;
  const currentSheetIds = (state.schematic?.sheet_path ?? []).map((s) => s.id);

  const [form, setForm] = useState<SchPlotForm>(kept);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);
  // "Design variant:" (KiCad 25ec2335 `m_variantChoiceCtrl->Append( GetVariantNamesForUI() ); Select( 0 )`): the default at each opening, `plotOpts.m_variant` to kicad-cli's `--variant`
  const [variant, setVariant] = useState("");
  const variants = state.schematic?.variants ?? [];

  useEffect(() => {
    if (!open) return;
    setForm(kept);
    setResult(null);
    setVariant("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_SCH_PLOT_DIALOG_OPEN", open: false });
  const set = (patch: Partial<SchPlotForm>) =>
    setForm((prev) => {
      const next = { ...prev, ...patch };
      kept = next;
      return next;
    });
  const en = schPlotEnables(form);

  const plot = async (scope: SchPlotScope) => {
    setBusy(true);
    setResult(null);
    try {
      // the default font of Display Options (`SetDefaultFont( cfg->m_Appearance.default_font )`), when it is not the KiCad font
      const defaultFont = state.prefs.schDefaultFont && state.prefs.schDefaultFont !== "KiCad Font" ? { default_font: state.prefs.schDefaultFont } : {};
      const reply = await postSchPlot({ ...buildSchPlotRequest({ ...form, currentSheetIds }, scope), ...(variant ? { variant } : {}), ...defaultFont });
      const files = reply.files ?? [];
      // `SCH_PLOTTER`'s reporter ("Plotted to '%s'.", "Done.", and any warning) is what kicad-cli printed: the Messages panel shows it (r5 X5-1)
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? (reply.log ?? summarizeOutputs(files)) : "Plot failed."), files });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 600 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Plot Schematic</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <label style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6 }} title="Target directory for plot files. Can be absolute or relative to the schematic main file location.">
            Output directory:
            <input type="text" value={form.outputDirectory} placeholder={`export/kicad/sch-${form.format}/`} onChange={(e) => set({ outputDirectory: e.target.value })} style={{ flex: 1 }} data-testid="sch-plot-output-dir" />
            <button disabled title={`Not available: ${SCH_CLI_LIMITS.browse}`}>…</button>
          </label>
          <VariantChoice variants={variants} value={variant} onChange={setVariant} testId="sch-plot-variant" style={{ marginBottom: 10 }} />
          <div style={{ display: "flex", gap: 12 }}>
            <fieldset style={{ minWidth: 110 }}>
              <legend>Output Format</legend>
              {SCH_PLOT_FORMATS.map((f) => (
                <label key={f.id} style={{ display: "block" }}>
                  <input type="radio" name="sch-plot-format" checked={form.format === f.id} onChange={() => set({ format: f.id })} /> {f.label}
                </label>
              ))}
            </fieldset>
            <div style={{ flex: 1 }}>
              <fieldset style={{ marginBottom: 8 }}>
                <legend>Options</legend>
                <div style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 8px", alignItems: "center" }}>
                  <span style={{ opacity: 0.6 }}>Page size:</span>
                  <select disabled value={SCH_PAGE_SIZES[0]} title={`Not available: ${SCH_CLI_LIMITS.pageSize}`}>
                    {SCH_PAGE_SIZES.map((p) => (
                      <option key={p}>{p}</option>
                    ))}
                  </select>
                  <span />
                  <label title="Plot the drawing sheet border and title block">
                    <input type="checkbox" checked={form.plotDrawingSheet} onChange={(e) => set({ plotDrawingSheet: e.target.checked })} /> Plot drawing sheet
                  </label>
                  <span>Output mode:</span>
                  <select value={form.color ? "color" : "bw"} onChange={(e) => set({ color: e.target.value === "color" })} data-testid="sch-plot-mode">
                    <option value="color">Color</option>
                    <option value="bw">Black and White</option>
                  </select>
                  <span style={{ opacity: en.theme ? 1 : 0.5 }}>Color theme:</span>
                  <select value={form.theme} disabled={!en.theme} title="Select the color theme to use for plotting" onChange={(e) => set({ theme: e.target.value })} data-testid="sch-plot-theme">
                    {SCH_COLOR_THEMES.map((t) => (
                      <option key={t.key} value={t.key}>
                        {t.name}
                      </option>
                    ))}
                  </select>
                  <span />
                  <label style={{ opacity: en.background ? 1 : 0.5 }} title="Plot the background color if the output format supports it">
                    <input type="checkbox" disabled={!en.background} checked={form.useBackgroundColor} onChange={(e) => set({ useBackgroundColor: e.target.checked })} /> Plot background color
                  </label>
                  <span style={{ opacity: 0.6 }}>Minimum line width:</span>
                  <span style={{ opacity: 0.6 }} title={`Selection of the default pen thickness used to draw items, when their thickness is set to 0. -- not available: ${SCH_CLI_LIMITS.lineWidth}`}>
                    <input type="text" disabled value="0.1524" size={8} readOnly /> mm
                  </span>
                </div>
              </fieldset>
              <fieldset style={{ marginBottom: 8, opacity: en.pdfOptions ? 1 : 0.5 }}>
                <legend>PDF Options</legend>
                <label style={{ display: "block" }}>
                  <input type="checkbox" disabled={!en.pdfOptions} checked={form.pdfPropertyPopups} onChange={(e) => set({ pdfPropertyPopups: e.target.checked })} /> Generate property popups
                </label>
                <label style={{ display: "block" }}>
                  <input type="checkbox" disabled={!en.pdfOptions} checked={form.pdfHierarchicalLinks} onChange={(e) => set({ pdfHierarchicalLinks: e.target.checked })} /> Generate clickable links for hierarchical elements
                </label>
                <label style={{ display: "block" }} title="Generate PDF document properties from AUTHOR and SUBJECT text variables">
                  <input type="checkbox" disabled={!en.pdfOptions} checked={form.pdfMetadata} onChange={(e) => set({ pdfMetadata: e.target.checked })} /> Generate metadata from AUTHOR &amp;&amp; SUBJECT variables
                </label>
              </fieldset>
              <fieldset style={{ marginBottom: 8, opacity: 0.6 }}>
                <legend>DXF Options</legend>
                <label title={`Not available: ${SCH_CLI_LIMITS.dxfUnits}`}>
                  Export units:{" "}
                  <select disabled value="mm">
                    <option value="in">Inches</option>
                    <option value="mm">Millimeters</option>
                  </select>
                </label>
              </fieldset>
              {en.pngOptions && (
                <fieldset style={{ marginBottom: 8 }}>
                  <legend>PNG Options</legend>
                  <label>
                    DPI:{" "}
                    <input type="number" min={72} max={2400} value={form.pngDpi} onChange={(e) => set({ pngDpi: Math.min(2400, Math.max(72, Number(e.target.value) || 300)) })} style={{ width: 70 }} />
                  </label>{" "}
                  <label>
                    <input type="checkbox" checked={form.pngAntialias} onChange={(e) => set({ pngAntialias: e.target.checked })} /> Anti-alias
                  </label>
                </fieldset>
              )}
              <fieldset style={{ marginBottom: 8, opacity: 0.6 }}>
                <legend>Other Options</legend>
                <label title={`Open output file with associated application after successful plot -- not available: ${SCH_CLI_LIMITS.openFile}`}>
                  <input type="checkbox" disabled checked={false} readOnly /> Open file after plot
                </label>
              </fieldset>
            </div>
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
          {!result && <div style={dim}>Written by kicad-cli sch export, KiCad's own plotter.</div>}
        </div>
        <div className="dialog-footer">
          <button className="primary" onClick={() => plot("all")} disabled={busy}>
            {busy ? "Plotting…" : "Plot All Pages"}
          </button>
          <button onClick={() => plot("current")} disabled={busy}>
            Plot Current Page
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
