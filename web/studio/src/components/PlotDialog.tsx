// dialog_plot.cpp ("Plot", File > Fabrication Outputs > Gerbers... and File > Plot...), with the controls, labels and defaults of
// dialog_plot_base.cpp (KiCad 8303b2ad): "Plot format:" (Gerber, Postscript, SVG, DXF, PDF, PNG), "Design variant:", "Output directory:",
// "Include Layers" and "Plot on All Layers" (with their right-click menus and the up/down buttons), the General Options, the solder mask alert
// with its Board setup link, each format's options box, the messages, "Run DRC..." with the known violations and exclusions, Plot,
// "Generate Drill Files..." and Close. The logic is kicad-port/plotDialog.ts (TransferDataToWindow, SetPlotFormat, applyPlotSettings).
//
// Plot first stores the controls in the board's plot settings (`applyPlotSettings`: `SetPlotSettings( tempOptions ); OnModify()` when they
// changed) with the `set_plot_settings` verb, which is no undo step (KiCad has no commit there; round 4 X4-3), then POSTs to /api/fab/plot
// (crates/cli/src/fab_api.rs), which runs `kicad-cli pcb export <format>` with the board's settings mapped onto its flags, into the board's
// output folder (empty: export/kicad/<format>/). The controls kicad-cli has no flag for are shown disabled with the reason.
import { useEffect, useRef, useState } from "react";
import { postFabPlot } from "../api/client";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength } from "../state/units";
import { enabledOptionalLayers, layerDisplayName } from "../kicad-port/boardLayers";
import {
  ALL_LAYERS_MENU,
  applyLayerMenu,
  arrangeAllLayers,
  constrainPostscript,
  COORD_FORMATS,
  DEFAULT_SESSION,
  DRILL_MARKS,
  drcWarning,
  initialPlotForm,
  KICAD_CLI_LIMITS,
  LAYER_MENU,
  PLOT_FORMATS,
  plotEnables,
  plotRequest,
  plotSettingsCommand,
  SCALES,
  showSolderMaskAlert,
  stackupForPlotting,
  uiOrder,
  withDxfContours,
  withFormat,
  type AllLayersMenuItem,
  type LayerMenuItem,
  type PcbPlotFormat,
  type PlotForm,
  type PlotSessionValues,
} from "../kicad-port/plotDialog";

/** What KiCad keeps for the session and not in the board (`s_lastAllLayersOrder`, the members `Format` does not write, `DIALOG_SHIM`'s saved
 * text controls): kept here while the page lives. */
let session: PlotSessionValues = DEFAULT_SESSION;

const dim = { color: "var(--chrome-text-dim)", fontSize: 11 } as const;
const boxStyle = { border: "1px solid var(--chrome-border)", borderRadius: 6, padding: "6px 8px", marginBottom: 8 } as const;

function Box({ title, children, testid }: { title: string; children: React.ReactNode; testid?: string }) {
  return (
    <fieldset style={boxStyle} data-testid={testid}>
      <legend style={dim}>{title}</legend>
      {children}
    </fieldset>
  );
}

function Check({ label, checked, onChange, disabled, title, testid }: { label: string; checked: boolean; onChange?: (v: boolean) => void; disabled?: boolean; title?: string; testid?: string }) {
  return (
    <label className="toggle" style={{ display: "block", opacity: disabled ? 0.5 : 1 }} title={title}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange?.(e.target.checked)} data-testid={testid} />
      {label}
    </label>
  );
}

export function PlotDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const copperLayers = state.board?.layers ?? ["F.Cu", "B.Cu"];
  const layerSetup = state.board?.board_rules?.layer_setup;
  // `m_layerList = board->GetEnabledLayers().UIOrder()`.
  const listed = uiOrder([...copperLayers, ...enabledOptionalLayers(layerSetup)]);
  const board = state.board?.plot_params ?? null;
  const [form, setForm] = useState<PlotForm>(() => initialPlotForm(board, listed, session));
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);
  const [menu, setMenu] = useState<{ list: "layers" | "all"; x: number; y: number } | null>(null);
  const [selectedAll, setSelectedAll] = useState<string | null>(null);
  const open = state.plotDialogOpen;

  // KiCad builds the dialog at each opening (`TransferDataToWindow`), so it starts from the board as it is then.
  useEffect(() => {
    if (!open) return;
    setForm(initialPlotForm(state.board?.plot_params ?? null, listed, session));
    setResult(null);
    setMenu(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // The dialogs it opens (Generate Drill Files, Run DRC, Board setup) are modal over it; on their return `reInitDialog` re-reads the output folder and
  // the origin (`m_plotOpts = GetPlotSettings()` after the drill dialog), and the DRC line and the mask alert, which are drawn from the state here.
  const child = state.generateDrillDialogOpen || state.drcDialogOpen || state.boardSetupDialogOpen;
  const wasChild = useRef(false);
  useEffect(() => {
    if (open && wasChild.current && !child) {
      const p = state.board?.plot_params;
      setForm((f) => ({ ...f, outputDirectory: p?.options?.outputdirectory ?? f.outputDirectory, useAuxOrigin: f.format === "ps" ? false : (p?.use_aux_origin ?? f.useAuxOrigin) }));
    }
    wasChild.current = child;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [child]);

  if (!open) return null;

  const set = (patch: Partial<PlotForm>) => setForm((f) => ({ ...f, ...patch }));
  const en = plotEnables(form);
  const rules = state.board?.board_rules;
  const keepSession = (f: PlotForm) => {
    session = { svgFitPage: f.svgFitPage, pdfBackground: f.pdfBackground, dxfSingle: f.dxfSingle, psXScale: f.psXScale, psYScale: f.psYScale, psWidthAdjust: f.psWidthAdjust, zoneFillCheck: f.zoneFillCheck, lastAllLayersOrder: f.allLayersOrder };
  };
  const close = () => {
    // `~DIALOG_PLOT`: `s_lastAllLayersOrder` is the list's order as it closes.
    keepSession(form);
    dispatch({ type: "SET_PLOT_DIALOG_OPEN", open: false });
  };

  /** `applyPlotSettings`: the PostScript values constrained (their messages for the panel), and the board's settings stored when they changed. */
  const apply = async (): Promise<{ ps: { x: number; y: number; widthMm: number }; messages: string[] } | null> => {
    const smallestClearance = Math.min(rules?.default_class?.clearance ?? rules?.clearance ?? 0, ...(rules?.net_classes ?? []).map((c) => c.clearance ?? Number.MAX_SAFE_INTEGER));
    const range: [number, number] = [-((rules?.constraints?.min_track_width_nm ?? 0) - 1), smallestClearance - 1];
    const ps = constrainPostscript(form, range, (nm) => formatLength(nm, state.units));
    const next = ps.form;
    setForm(next);
    keepSession(next);
    const cmd = plotSettingsCommand(next, state.board?.plot_params ?? null, listed);
    if (cmd && !(await api.cmd(cmd))) return null;
    return { ps, messages: ps.messages };
  };

  const plot = async () => {
    setBusy(true);
    setResult(null);
    try {
      const applied = await apply();
      if (!applied) return;
      // `if( !m_plotOpts.GetLayerSelection().any() ) DisplayError( this, _( "No layer selected, Nothing to plot" ) )`.
      if (form.checked.length === 0) {
        setResult({ ok: false, message: "No layer selected, Nothing to plot", files: [] });
        return;
      }
      const reply = await postFabPlot(plotRequest(form, applied.ps));
      const message = [...applied.messages, reply.message ?? (reply.ok ? "Done." : "Plot failed.")].join("\n");
      setResult({ ok: reply.ok, message, files: reply.files ?? [] });
    } finally {
      setBusy(false);
    }
  };

  // `CreateDrillFile`: the settings first, so the drill files use the same origin and folder, then Generate Drill Files over this dialog.
  const drill = async () => {
    if (await apply()) dispatch({ type: "SET_GENERATE_DRILL_DIALOG_OPEN", open: true });
  };

  const toggleLayer = (layer: string) => setForm((f) => ({ ...f, checked: f.checked.includes(layer) ? f.checked.filter((l) => l !== layer) : uiOrder([...f.checked, layer]) }));
  const toggleAll = (layer: string) => setForm((f) => ({ ...f, allLayersChecked: f.allLayersChecked.includes(layer) ? f.allLayersChecked.filter((l) => l !== layer) : [...f.allLayersChecked, layer] }));
  const moveAll = (delta: -1 | 1) =>
    setForm((f) => {
      if (!selectedAll) return f;
      const i = f.allLayersOrder.indexOf(selectedAll);
      const j = i + delta;
      if (i < 0 || j < 0 || j >= f.allLayersOrder.length) return f;
      const order = [...f.allLayersOrder];
      [order[i], order[j]] = [order[j] as string, order[i] as string];
      return { ...f, allLayersOrder: order };
    });
  const layerMenu = (item: LayerMenuItem) => setForm((f) => ({ ...f, checked: applyLayerMenu(item, listed, f.checked) }));
  const allMenu = (item: AllLayersMenuItem) => {
    if (item === "stackup") setSelectedAll(null);
    setForm((f) => {
      if (item === "selectAll") return { ...f, allLayersChecked: [...f.allLayersOrder] };
      if (item === "deselectAll") return { ...f, allLayersChecked: [] };
      return { ...f, allLayersOrder: arrangeAllLayers(f.allLayersOrder, stackupForPlotting(listed)) };
    });
  };

  const drc = drcWarning(state.drc ? [state.drc.violations, state.drc.unconnected_items, state.drc.schematic_parity] : null);
  const maskAlert = showSolderMaskAlert(form.format, rules?.mask_paste);
  const name = (l: string) => layerDisplayName(layerSetup, l);
  const f = form;
  const pdfPopup = (v: boolean) => set({ pdfFrontPopups: v, pdfBackPopups: v });

  return (
    <div className="dialog-backdrop" onClick={close} style={child ? { display: "none" } : undefined}>
      <div className="dialog" style={{ width: 960, maxWidth: "96vw", maxHeight: "90vh" }} onClick={(e) => (e.stopPropagation(), setMenu(null))}>
        <div className="dialog-header">Plot</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "5px 8px", alignItems: "center", marginBottom: 8 }}>
            <span>Plot format:</span>
            <span style={{ display: "flex", gap: 28, alignItems: "center" }}>
              <select value={f.format} onChange={(e) => setForm((x) => withFormat(x, e.target.value as PcbPlotFormat))} data-testid="plot-format">
                {PLOT_FORMATS.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.label}
                  </option>
                ))}
              </select>
              <label style={{ display: "flex", gap: 6, alignItems: "center", opacity: 0.6 }} title={`Not available: ${KICAD_CLI_LIMITS.variant}`}>
                Design variant:
                <select disabled value="">
                  <option value="">Default</option>
                </select>
              </label>
            </span>
            <span>Output directory:</span>
            <span style={{ display: "flex", gap: 4 }}>
              <input
                type="text"
                value={f.outputDirectory}
                placeholder={`export/kicad/${f.format === "gerber" ? "gerbers" : f.format}/`}
                title="Target directory for plot files. Can be absolute or relative to the board file location."
                onChange={(e) => set({ outputDirectory: e.target.value })}
                style={{ flex: 1 }}
                data-testid="plot-output-dir"
              />
              <button disabled title={`Not available: ${KICAD_CLI_LIMITS.browse}`}>…</button>
              <button disabled title={`Open output directory -- not available: ${KICAD_CLI_LIMITS.openDir}`}>↗</button>
            </span>
          </div>

          <div style={{ display: "flex", gap: 10, alignItems: "stretch" }}>
            <Box title="Include Layers" testid="plot-include-layers">
              <div style={{ maxHeight: 330, overflowY: "auto", minWidth: 150 }} onContextMenu={(e) => (e.preventDefault(), setMenu({ list: "layers", x: e.clientX, y: e.clientY }))}>
                {listed.map((layer) => (
                  <label key={layer} className="toggle" style={{ display: "block" }} data-layer={layer}>
                    <input type="checkbox" checked={f.checked.includes(layer)} onChange={() => toggleLayer(layer)} />
                    {name(layer)}
                  </label>
                ))}
              </div>
            </Box>
            <Box title="Plot on All Layers" testid="plot-all-layers">
              <div style={{ maxHeight: 300, overflowY: "auto", minWidth: 150 }} onContextMenu={(e) => (e.preventDefault(), setMenu({ list: "all", x: e.clientX, y: e.clientY }))}>
                {f.allLayersOrder.map((layer) => (
                  <div key={layer} onClick={() => setSelectedAll(layer)} style={{ background: selectedAll === layer ? "var(--chrome-bg)" : undefined }} data-layer={layer}>
                    <label className="toggle">
                      <input type="checkbox" checked={f.allLayersChecked.includes(layer)} onChange={() => toggleAll(layer)} />
                      {name(layer)}
                    </label>
                  </div>
                ))}
              </div>
              <div style={{ display: "flex", gap: 3, marginTop: 4 }}>
                <button title="Move current selection up" onClick={() => moveAll(-1)} disabled={!selectedAll}>
                  ▲
                </button>
                <button title="Move current selection down" onClick={() => moveAll(1)} disabled={!selectedAll}>
                  ▼
                </button>
              </div>
            </Box>
            <div style={{ flex: 1, minWidth: 0 }}>
              <Box title="General Options">
                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "3px 12px" }}>
                  <div>
                    <Check label="Plot drawing sheet" checked={f.plotSheetRef} onChange={(v) => set({ plotSheetRef: v })} />
                    <Check label="Subtract soldermask from silkscreen" title="Remove silkscreen from areas without soldermask" checked={f.subtractMaskFromSilk} onChange={(v) => set({ subtractMaskFromSilk: v })} />
                    <Check label="Indicate DNP on fabrication layers" title="Hide or cross-out DNP footprints on fabrication layers" checked={f.plotDNP} onChange={(v) => set({ plotDNP: v })} testid="plot-dnp" />
                    <div style={{ paddingLeft: 20, opacity: en.dnpRadios ? 1 : 0.5 }}>
                      <label className="toggle" title="Hide the footprint text and graphics">
                        <input type="radio" name="plot-dnp" disabled={!en.dnpRadios} checked={f.dnpMode === "hide"} onChange={() => set({ dnpMode: "hide" })} />
                        Hide
                      </label>
                      <label className="toggle">
                        <input type="radio" name="plot-dnp" disabled={!en.dnpRadios} checked={f.dnpMode === "crossout"} onChange={() => set({ dnpMode: "crossout" })} />
                        Cross-out
                      </label>
                    </div>
                    <Check label="Sketch pads on fabrication layers" title="Include pad outlines on F.Fab and B.Fab layers when plotting" checked={f.sketchPads} onChange={(v) => set({ sketchPads: v })} />
                    <div style={{ paddingLeft: 20 }}>
                      <Check label="Include pad numbers" checked={f.padNumbers} disabled={!en.padNumbers} onChange={(v) => set({ padNumbers: v })} />
                    </div>
                    <Check label="Check zone fills before plotting" title="Run KiCad's zone filler first if some zone has no current fill (never filled, unfilled, or edited since)" checked={f.zoneFillCheck} onChange={(v) => set({ zoneFillCheck: v })} testid="plot-check-zones" />
                  </div>
                  <div>
                    <label style={{ display: "flex", gap: 6, alignItems: "center", opacity: en.drillMarks ? 1 : 0.5 }}>
                      Drill marks:
                      <select value={f.drillMarks} disabled={!en.drillMarks} onChange={(e) => set({ drillMarks: Number(e.target.value) })} data-testid="plot-drill-marks">
                        {DRILL_MARKS.map((m, i) => (
                          <option key={m} value={i}>
                            {m}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label style={{ display: "flex", gap: 6, alignItems: "center", marginTop: 3, opacity: en.scale ? 1 : 0.5 }}>
                      Scaling:
                      <select value={f.scale} disabled={!en.scale} onChange={(e) => set({ scale: Number(e.target.value) })} data-testid="plot-scale">
                        {SCALES.map((m, i) => (
                          <option key={m} value={i}>
                            {m}
                          </option>
                        ))}
                      </select>
                    </label>
                    <Check
                      label="Use drill/place file origin"
                      title={en.auxOriginCliReason ? `Not available: ${en.auxOriginCliReason}` : "Use the drill/place file origin as the coordinate origin for plotted files"}
                      checked={f.useAuxOrigin}
                      disabled={!en.useAuxOrigin}
                      onChange={(v) => set({ useAuxOrigin: v })}
                    />
                    <Check label="Mirrored plot" checked={f.mirror} disabled={!en.mirror} onChange={(v) => set({ mirror: v })} />
                    <Check label="Negative plot" checked={f.negative} disabled={!en.negative} onChange={(v) => set({ negative: v })} />
                  </div>
                </div>
              </Box>
              {maskAlert && (
                <div className="problem-row" style={{ fontSize: 11, marginBottom: 8 }} data-testid="plot-mask-alert">
                  <div>Global solder mask minimum width and/or margin are not set to 0. </div>
                  <div>
                    Most board manufacturers expect 0 and use their own constraints for solder mask minimum width.{" "}
                    <a
                      href="#"
                      title="File > Board Setup..."
                      onClick={(e) => {
                        e.preventDefault();
                        dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: "mask_paste" });
                        dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true });
                      }}
                    >
                      Board setup
                    </a>
                  </div>
                </div>
              )}
              {f.format === "gerber" && (
                <Box title="Gerber Options" testid="plot-gerber-options">
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "3px 12px" }}>
                    <div>
                      <Check label="Use Protel filename extensions" title={"Use Protel Gerber extensions (.GBL, .GTL, etc...)\nNo longer recommended. The official extension is .gbr"} checked={f.protelExt} onChange={(v) => set({ protelExt: v })} />
                      <Check
                        label="Generate Gerber job file"
                        title={"Generate a Gerber job file that contains info about the board,\nand the list of generated Gerber plot files"}
                        checked={f.jobFile}
                        onChange={(v) => set({ jobFile: v })}
                        testid="plot-job-file"
                      />
                      <label style={{ display: "flex", gap: 6, alignItems: "center", marginTop: 3 }}>
                        Coordinate format:
                        <select value={f.coordFormat} onChange={(e) => set({ coordFormat: Number(e.target.value) })}>
                          {COORD_FORMATS.map((c, i) => (
                            <option key={c} value={5 + i}>
                              {c}
                            </option>
                          ))}
                        </select>
                      </label>
                    </div>
                    <div>
                      <Check
                        label="Use extended X2 format (recommended)"
                        title={"Use X2 Gerber file format.\nInclude  mainly X2 attributes in Gerber headers.\nIf not checked, use X1 format.\nIn X1 format, these attributes are included as comments in files."}
                        checked={f.x2}
                        onChange={(v) => set({ x2: v })}
                      />
                      <Check
                        label="Include netlist attributes"
                        title={"Include netlist metadata and aperture attributes in Gerber files.\nThey are comments in the X1 format.\nUsed to check connectivity in CAM tools and Gerber viewers."}
                        checked={f.netAttributes}
                        onChange={(v) => set({ netAttributes: v })}
                      />
                      <Check label="Disable aperture macros (not recommended)" title={"Disable aperture macros in Gerber files\nUse *only* for broken Gerber viewers."} checked={f.disableMacros} onChange={(v) => set({ disableMacros: v })} />
                    </div>
                  </div>
                </Box>
              )}
              {f.format === "ps" && (
                <Box title="Postscript Options" testid="plot-ps-options">
                  <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
                    <label title="Set global X scale adjust for exact scale PostScript output.">
                      X scale factor: <input type="text" size={8} value={f.psXScale} onChange={(e) => set({ psXScale: e.target.value })} />
                    </label>
                    <label title="Set global Y scale adjust for exact scale PostScript output.">
                      Y scale factor: <input type="text" size={8} value={f.psYScale} onChange={(e) => set({ psYScale: e.target.value })} />
                    </label>
                    <label title={"Set global width correction for exact width PostScript output.\nThese width correction is intended to compensate tracks width and also pads and vias size errors.\nThe reasonable width correction value must be in a range of [-(MinTrackWidth-1), +(MinClearanceValue-1)]."}>
                      Track width correction: <input type="text" size={8} value={f.psWidthAdjust} onChange={(e) => set({ psWidthAdjust: e.target.value })} /> mm
                    </label>
                  </div>
                  <Check label="Force A4 output" checked={f.forceA4} disabled={!en.forceA4} onChange={(v) => set({ forceA4: v })} />
                </Box>
              )}
              {f.format === "dxf" && (
                <Box title="DXF Options" testid="plot-dxf-options">
                  <Check label="Plot graphic items using their contours" title="Uncheck to plot graphic items using their center lines" checked={f.dxfContours} onChange={(v) => setForm((x) => withDxfContours(x, v))} />
                  <label style={{ display: "flex", gap: 6, alignItems: "center" }} title="The units to use for the exported DXF file">
                    Export units:
                    <select value={f.dxfUnits} onChange={(e) => set({ dxfUnits: e.target.value as "in" | "mm" })}>
                      <option value="in">Inches</option>
                      <option value="mm">Millimeters</option>
                    </select>
                  </label>
                  <Check label="Use KiCad font to plot text" title={`Not available: ${KICAD_CLI_LIMITS.dxfFont}`} checked={f.dxfKiCadFont} disabled />
                  <Check label="Single document" title="Export selected layers into a single DXF" checked={f.dxfSingle} onChange={(v) => set({ dxfSingle: v })} />
                </Box>
              )}
              {f.format === "svg" && (
                <Box title="SVG Options" testid="plot-svg-options">
                  <label style={{ display: "flex", gap: 6, alignItems: "center", opacity: 0.6 }} title={`Not available: ${KICAD_CLI_LIMITS.svgPrecision}`}>
                    Precision:
                    <input type="number" min={3} max={6} value={f.svgPrecision} disabled readOnly style={{ width: 50 }} />
                  </label>
                  <label style={{ display: "flex", gap: 6, alignItems: "center", marginTop: 3 }}>
                    Output mode:
                    <select value={f.svgBlackAndWhite ? "bw" : "color"} onChange={(e) => set({ svgBlackAndWhite: e.target.value === "bw" })}>
                      <option value="color">Color</option>
                      <option value="bw">Black and white</option>
                    </select>
                  </label>
                  <Check label="Fit page to board" checked={f.svgFitPage} onChange={(v) => set({ svgFitPage: v })} />
                </Box>
              )}
              {f.format === "pdf" && (
                <Box title="PDF Options" testid="plot-pdf-options">
                  <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
                    Output mode:
                    <select value={f.pdfBlackAndWhite ? "bw" : "color"} onChange={(e) => set({ pdfBlackAndWhite: e.target.value === "bw" })} data-testid="plot-pdf-color">
                      <option value="color">Color</option>
                      <option value="bw">Black and white</option>
                    </select>
                  </label>
                  <Check label="Generate property popups for front footprints" title={KICAD_CLI_LIMITS.pdfPopups} checked={f.pdfFrontPopups} onChange={pdfPopup} />
                  <Check label="Generate property popups for back footprints" title={KICAD_CLI_LIMITS.pdfPopups} checked={f.pdfBackPopups} onChange={pdfPopup} />
                  <Check label="Generate metadata from AUTHOR && SUBJECT variables" title={`Generate PDF document properties from AUTHOR and SUBJECT text variables -- not available: ${KICAD_CLI_LIMITS.pdfMetadata}`} checked disabled />
                  <Check label="Single document" checked={f.pdfSingle} onChange={(v) => set({ pdfSingle: v })} testid="plot-pdf-single" />
                  <label style={{ display: "flex", gap: 6, alignItems: "center", opacity: en.pdfBackground ? 1 : 0.5 }}>
                    Background Color
                    <input type="color" disabled={!en.pdfBackground} value={f.pdfBackground ?? "#ffffff"} onChange={(e) => set({ pdfBackground: e.target.value })} />
                    <button disabled={!en.pdfBackground || f.pdfBackground === null} onClick={() => set({ pdfBackground: null })} title="No background colour (COLOR4D::UNSPECIFIED)">
                      None
                    </button>
                  </label>
                </Box>
              )}
              {f.format === "png" && (
                <Box title="PNG Options" testid="plot-png-options">
                  <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
                    DPI:
                    <input type="number" min={72} max={2400} value={f.pngDpi} onChange={(e) => set({ pngDpi: Math.min(2400, Math.max(72, Number(e.target.value) || 300)) })} style={{ width: 70 }} />
                  </label>
                  <Check label="Anti-alias" checked={f.pngAntialias} onChange={(v) => set({ pngAntialias: v })} />
                </Box>
              )}
            </div>
          </div>

          {result && (
            <div className={result.ok ? "panel-empty" : "problem-row"} style={{ fontSize: 11 }}>
              <b style={{ whiteSpace: "pre-line" }}>{result.message}</b>
              {result.files.length > 0 && (
                <ul style={{ margin: "6px 0 0 16px", padding: 0 }}>
                  {result.files.map((file) => (
                    <li key={file}>{file}</li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
        <div className="dialog-footer" style={{ justifyContent: "space-between" }}>
          <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <button onClick={() => dispatch({ type: "SET_DRC_OPEN", open: true })}>Run DRC...</button>
            {drc && (
              <span style={dim} data-testid="plot-drc-warning">
                {drc}
              </span>
            )}
          </span>
          <span style={{ display: "flex", gap: 8 }}>
            <button className="primary" onClick={plot} disabled={busy}>
              {busy ? "Plotting…" : "Plot"}
            </button>
            <button onClick={drill} disabled={busy}>
              Generate Drill Files...
            </button>
            <button onClick={close}>Close</button>
          </span>
        </div>
      </div>
      {menu && (
        <div className="context-menu" style={{ position: "fixed", left: menu.x, top: menu.y, zIndex: 2100, background: "var(--chrome-bg-raised)", border: "1px solid var(--chrome-border)", borderRadius: 6, padding: 4 }} onClick={(e) => e.stopPropagation()}>
          {(menu.list === "layers" ? LAYER_MENU : ALL_LAYERS_MENU).map((item, i) =>
            item ? (
              <div
                key={item.id}
                className="context-menu-item"
                style={{ padding: "3px 10px", cursor: "pointer" }}
                onClick={() => {
                  if (menu.list === "layers") layerMenu(item.id as LayerMenuItem);
                  else allMenu(item.id as AllLayersMenuItem);
                  setMenu(null);
                }}
              >
                {item.label}
              </div>
            ) : (
              <hr key={`sep${i}`} style={{ margin: "3px 0", border: 0, borderTop: "1px solid var(--chrome-border)" }} />
            ),
          )}
        </div>
      )}
    </div>
  );
}
