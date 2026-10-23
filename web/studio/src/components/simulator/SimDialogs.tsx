// The simulator's dialogs: the analysis dialog (`DIALOG_SIM_COMMAND`, eeschema/dialogs/dialog_sim_command.cpp and its _base), User-defined Signals
// (`DIALOG_USER_DEFINED_SIGNALS`), SPICE Netlist (`SIMULATOR_CONTROL`'s `NETLIST_VIEW_DIALOG`), Format Value (`DIALOG_SIM_FORMAT_VALUE`), the Fourier
// fundamental (`WX_TEXT_ENTRY_DIALOG`), Save Simulation Workbook As and the frame's questions and messages.
import { useState } from "react";
import {
  NEW_TAB_TYPES,
  OPTION_ADJUST_INCLUDE_PATHS,
  OPTION_SAVE_ALL_CURRENTS,
  OPTION_SAVE_ALL_DISSIPATIONS,
  OPTION_SAVE_ALL_EVENTS,
  OPTION_SAVE_ALL_VOLTAGES,
  buildCommand,
  dcUnit,
  longName,
  optionsOf,
  shortName,
  commandToSimType,
  type CommandFields,
  type SimType,
} from "../../kicad-port/sim/simCommand";
import { currentTab, setUserSignals } from "../../kicad-port/sim/simFrame";
import { ensureThirdYAxis, gridShown, labelOf, setGrid, unitsOf, type PlotTab } from "../../kicad-port/sim/simPlotTab";
import { adjustLimitedView } from "../../kicad-port/sim/mathPlot";
import { SpiceValue, formatToString, type SpiceValueFormat } from "../../kicad-port/sim/spiceValue";
import { applyAnalysis, doFourier, getSimUi, rerunIfFinished, saveWorkbookAs, setSimDialog, touch, useSimUi } from "../../state/simulator";
import { sortSignals, signalMatches } from "../../kicad-port/sim/simSignals";

function Shell({ title, width = 520, onCancel, onOk, okLabel = "OK", children, footer }: { title: string; width?: number; onCancel: () => void; onOk?: () => void; okLabel?: string; children: React.ReactNode; footer?: React.ReactNode }) {
  return (
    <div className="dialog-backdrop sim-dialog" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className="dialog" role="dialog" aria-label={title} style={{ width }} onKeyDown={(e) => {
        if (e.key === "Escape") onCancel();
      }}>
        <div className="dialog-header">{title}</div>
        <div className="dialog-body">{children}</div>
        <div className="dialog-footer">
          {footer}
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          {onOk && (
            <button type="button" className="primary" onClick={onOk}>
              {okLabel}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

const MODES = ["User configuration", "Spice", "PSpice", "LTSpice", "PSpice and LTSpice", "HSpice"];

function Row({ label, children, unit }: { label: string; children: React.ReactNode; unit?: string }) {
  return (
    <label className="sim-row">
      <span className="sim-row-label">{label}</span>
      {children}
      {unit !== undefined && <span className="sim-row-unit">{unit}</span>}
    </label>
  );
}

/** `DIALOG_SIM_COMMAND`: the SPICE Command page and, for a plot, the Plot Setup page. */
function CommandDialog({ d }: { d: Extract<NonNullable<ReturnType<typeof useSimUi>["dialog"]>, { kind: "command" }> }) {
  const ui = useSimUi();
  const circuit = ui.frame.circuit;
  const tab = d.mode === "edit" ? currentTab(ui.frame) : null;
  const plot = tab?.plot ?? null;
  const [f, setF] = useState<CommandFields>(d.fields);
  const [page, setPage] = useState<"command" | "plot">("command");
  const [opts, setOpts] = useState({
    fixIncludePaths: !!(d.options & OPTION_ADJUST_INCLUDE_PATHS),
    saveAllVoltages: !!(d.options & OPTION_SAVE_ALL_VOLTAGES),
    saveAllCurrents: !!(d.options & OPTION_SAVE_ALL_CURRENTS),
    saveAllDissipations: !!(d.options & OPTION_SAVE_ALL_DISSIPATIONS),
    saveAllEvents: !!(d.options & OPTION_SAVE_ALL_EVENTS),
  });
  const [mode, setMode] = useState(ui.settings.model_mode);
  const [error, setError] = useState<string | null>(null);
  const scaleText = (s: PlotTab["y1"]) => (s && (s.locked ? [s.axisMin, s.axisMax] : null)) || null;
  const [plotSet, setPlotSet] = useState(() => ({
    lock1: !!plot?.y1?.locked,
    min1: scaleText(plot?.y1 ?? null)?.[0]?.toString() ?? "",
    max1: scaleText(plot?.y1 ?? null)?.[1]?.toString() ?? "",
    lock2: !!plot?.y2?.locked,
    min2: scaleText(plot?.y2 ?? null)?.[0]?.toString() ?? "",
    max2: scaleText(plot?.y2 ?? null)?.[1]?.toString() ?? "",
    lock3: !!plot?.y3?.locked,
    min3: scaleText(plot?.y3 ?? null)?.[0]?.toString() ?? "",
    max3: scaleText(plot?.y3 ?? null)?.[1]?.toString() ?? "",
    grid: plot ? gridShown(plot) : true,
    legend: !!plot?.legend.shown,
    dotted: plot ? plot.dotted : true,
    left: `${plot?.view.marginLeft ?? 70}`,
    top: `${plot?.view.marginTop ?? 30}`,
    right: `${plot?.view.marginRight ?? 70}`,
    bottom: `${plot?.view.marginBottom ?? 45}`,
  }));
  const set = (patch: Partial<CommandFields>) => setF((x) => ({ ...x, ...patch }));
  const itemName = (ref: string) => circuit?.items.find((i) => i.ref === ref)?.name ?? "";
  const nets = circuit?.nets ?? [];
  const sources = (type: string) => (type === "TEMP" ? ["TEMP"] : [...new Set((circuit?.items ?? []).filter((i) => i.device === type.charAt(0)).map((i) => i.ref))].sort());
  const vSources = (circuit?.items ?? []).filter((i) => i.device === "V").map((i) => i.ref);
  const title = d.mode === "new" ? "New Simulation Tab" : `${shortName(commandToSimType(d.command))}  —  ${longName(commandToSimType(d.command))}`;
  const [fftFilter, setFftFilter] = useState("");

  const ok = () => {
    const r = buildCommand(f, itemName, d.command === "*" ? "" : d.command);
    if ("error" in r) {
      setError(r.error);
      return;
    }
    const options = optionsOf(opts);
    void applyAnalysis(d.mode, r.command, options, { ...ui.settings, model_mode: mode, fix_include_paths: opts.fixIncludePaths }, (t) => {
      const p = t.plot;
      if (!p) return;
      // `DIALOG_SIM_COMMAND::ApplySettings`
      const lock = (s: PlotTab["y1"], on: boolean, min: string, max: string) => {
        if (!s) return;
        s.locked = on;
        s.axisMin = SpiceValue.parse(min)?.toDouble() ?? 0;
        s.axisMax = SpiceValue.parse(max)?.toDouble() ?? 0;
      };
      if (labelOf(p.y1) !== "") lock(p.y1, plotSet.lock1, plotSet.min1, plotSet.max1);
      if (labelOf(p.y2) !== "") lock(p.y2, plotSet.lock2, plotSet.min2, plotSet.max2);
      if (plotSet.lock3 && !p.y3) ensureThirdYAxis(p);
      if (labelOf(p.y3) !== "") lock(p.y3, plotSet.lock3, plotSet.min3, plotSet.max3);
      p.view.yLocked = plotSet.lock1 || plotSet.lock2 || plotSet.lock3;
      setGrid(p, plotSet.grid);
      p.legend.shown = plotSet.legend;
      p.dotted = plotSet.dotted;
      p.view.marginLeft = Number.parseInt(plotSet.left, 10) || 0;
      p.view.marginRight = Number.parseInt(plotSet.right, 10) || 0;
      p.view.marginTop = Number.parseInt(plotSet.top, 10) || 0;
      p.view.marginBottom = Number.parseInt(plotSet.bottom, 10) || 0;
      adjustLimitedView(p.view);
    });
  };

  const scale = (key: "acScale" | "spScale" | "noiseScale") => (
    <Row label="Frequency scale">
      <select value={f[key]} onChange={(e) => set({ [key]: e.target.value } as Partial<CommandFields>)}>
        <option value="dec">Decade</option>
        <option value="oct">Octave</option>
        <option value="lin">Linear</option>
      </select>
    </Row>
  );
  const text = (key: keyof CommandFields, label: string, unit?: string) => (
    <Row label={label} unit={unit}>
      <input aria-label={label} value={String(f[key] ?? "")} onChange={(e) => set({ [key]: e.target.value } as Partial<CommandFields>)} />
    </Row>
  );
  const choice = (key: keyof CommandFields, label: string, options: string[], blank = false) => (
    <Row label={label}>
      <select aria-label={label} value={String(f[key] ?? "")} onChange={(e) => set({ [key]: e.target.value } as Partial<CommandFields>)}>
        {blank && <option value="" />}
        {options.map((o) => (
          <option key={o} value={o}>
            {o}
          </option>
        ))}
      </select>
    </Row>
  );
  const pointsLabel = (s: string) => (s === "lin" ? "Number of points:" : s === "oct" ? "Number of points per octave:" : "Number of points per decade:");

  const pages: Record<string, React.ReactNode> = {
    AC: (
      <>
        {scale("acScale")}
        {text("acPoints", pointsLabel(f.acScale))}
        {text("acStart", "Start frequency:", "Hz")}
        {text("acStop", "Stop frequency:", "Hz")}
      </>
    ),
    SP: (
      <>
        {scale("spScale")}
        {text("spPoints", pointsLabel(f.spScale))}
        {text("spStart", "Start frequency:", "Hz")}
        {text("spStop", "Stop frequency:", "Hz")}
        <label className="sim-check">
          <input type="checkbox" checked={f.spNoise} onChange={(e) => set({ spNoise: e.target.checked })} />
          Compute noise current correlation matrix
        </label>
      </>
    ),
    DC: (
      <div className="sim-dc">
        {([1, 2] as const).map((n) => {
          const type = n === 1 ? f.dcType1 : f.dcType2;
          const on = n === 1 || f.dcEnable2;
          return (
            <fieldset key={n} disabled={!on}>
              <legend>
                {n === 2 ? (
                  <label>
                    <input type="checkbox" checked={f.dcEnable2} onChange={(e) => set({ dcEnable2: e.target.checked })} /> Source 2
                  </label>
                ) : (
                  "Source 1"
                )}
              </legend>
              <Row label="Sweep type:">
                <select aria-label={`Source ${n} sweep type`} value={type} onChange={(e) => set(n === 1 ? { dcType1: e.target.value as CommandFields["dcType1"], dcSource1: e.target.value === "TEMP" ? "TEMP" : "" } : { dcType2: e.target.value as CommandFields["dcType2"], dcSource2: e.target.value === "TEMP" ? "TEMP" : "" })}>
                  {["V", "I", "R", "TEMP"].map((t) => (
                    <option key={t}>{t}</option>
                  ))}
                </select>
              </Row>
              <Row label="Source:">
                <select aria-label={`Source ${n}`} disabled={type === "TEMP"} value={n === 1 ? f.dcSource1 : f.dcSource2} onChange={(e) => set(n === 1 ? { dcSource1: e.target.value } : { dcSource2: e.target.value })}>
                  <option value="" />
                  {sources(type).map((s) => (
                    <option key={s}>{s}</option>
                  ))}
                </select>
              </Row>
              {text(n === 1 ? "dcStart1" : "dcStart2", "Starting value:", dcUnit(type))}
              {text(n === 1 ? "dcStop1" : "dcStop2", "Final value:", dcUnit(type))}
              {text(n === 1 ? "dcIncr1" : "dcIncr2", "Increment step:", dcUnit(type))}
            </fieldset>
          );
        })}
        <button type="button" onClick={() => setF((x) => ({ ...x, dcType1: x.dcType2, dcSource1: x.dcSource2, dcStart1: x.dcStart2, dcStop1: x.dcStop2, dcIncr1: x.dcIncr2, dcType2: x.dcType1, dcSource2: x.dcSource1, dcStart2: x.dcStart1, dcStop2: x.dcStop1, dcIncr2: x.dcIncr1 }))}>
          Swap sources
        </button>
      </div>
    ),
    TRAN: (
      <>
        {text("tranStep", "Time step:", "seconds")}
        {text("tranFinal", "Final time:", "seconds")}
        {text("tranInitial", "Initial time:", "seconds")}
        <div className="sim-hint">(optional; default 0)</div>
        {text("tranMaxStep", "Max time step:", "seconds")}
        <div className="sim-hint">(optional; default min{"{tstep, (tstop-tstart)/50}"})</div>
        <label className="sim-check">
          <input type="checkbox" checked={f.tranUic} onChange={(e) => set({ tranUic: e.target.checked })} />
          Use initial conditions
        </label>
      </>
    ),
    FFT: (
      <>
        <div>Input signals:</div>
        <input type="search" placeholder="Filter" aria-label="Filter input signals" value={fftFilter} onChange={(e) => setFftFilter(e.target.value)} />
        <div className="sim-checklist">
          {sortSignals([...ui.frame.signals, ...ui.frame.userSignals.values()])
            .filter((s) => signalMatches(fftFilter, s))
            .map((s) => (
              <label key={s} className="sim-check">
                <input type="checkbox" checked={f.fftSignals.includes(s)} onChange={(e) => set({ fftSignals: e.target.checked ? [...f.fftSignals, s] : f.fftSignals.filter((x) => x !== s) })} />
                {s}
              </label>
            ))}
        </div>
        <label className="sim-check">
          <input type="checkbox" checked={f.fftLinearize} onChange={(e) => set({ fftLinearize: e.target.checked })} />
          Linearize inputs before performing FFT
        </label>
      </>
    ),
    NOISE: (
      <>
        {choice("noiseMeas", "Measured node:", nets)}
        {choice("noiseRef", "Reference node:", nets, true)}
        <div className="sim-hint">(optional; default GND)</div>
        {choice("noiseSrc", "Noise source:", vSources)}
        {scale("noiseScale")}
        {text("noisePoints", pointsLabel(f.noiseScale))}
        {text("noiseStart", "Start frequency:", "Hz")}
        {text("noiseStop", "Stop frequency:", "Hz")}
        <label className="sim-check">
          <input type="checkbox" checked={f.noiseSaveAll} onChange={(e) => set({ noiseSaveAll: e.target.checked })} />
          Save contributions from all noise generators
        </label>
      </>
    ),
    PZ: (
      <>
        <Row label="Transfer function:">
          <select value={f.pzFunction} onChange={(e) => set({ pzFunction: e.target.value as "vol" | "cur" })}>
            <option value="vol">(output voltage) / (input voltage)</option>
            <option value="cur">(output voltage) / (input current)</option>
          </select>
        </Row>
        {choice("pzInput", "Input:", nets)}
        {choice("pzInputRef", "Ref:", nets)}
        {choice("pzOutput", "Output:", nets)}
        {choice("pzOutputRef", "Ref:", nets)}
        <Row label="Find:">
          <select value={f.pzAnalyses} onChange={(e) => set({ pzAnalyses: e.target.value as "pz" | "pol" | "zer" })}>
            <option value="pz">Poles and Zeros</option>
            <option value="pol">Poles</option>
            <option value="zer">Zeros</option>
          </select>
        </Row>
      </>
    ),
    OP: <div className="sim-hint">DC operating point analysis.</div>,
    UNKNOWN: (
      <>
        <div>SPICE directives:</div>
        <textarea className="sim-custom" aria-label="SPICE directives" value={f.custom} onChange={(e) => set({ custom: e.target.value })} rows={6} />
        <button type="button" onClick={() => set({ custom: circuit?.sch_sim_command ?? "" })}>
          Load Directives from Schematic
        </button>
      </>
    ),
  };

  const lockRow = (n: 1 | 2 | 3, s: PlotTab["y1"]) => {
    if (!s || labelOf(s) === "") return null;
    const k = n as 1 | 2 | 3;
    const lock = plotSet[`lock${k}`];
    return (
      <div className="sim-lock">
        <label className="sim-check">
          <input type="checkbox" checked={lock} onChange={(e) => setPlotSet((p) => ({ ...p, [`lock${k}`]: e.target.checked }))} />
          Fixed {labelOf(s)} scale
        </label>
        <Row label="Min:" unit={unitsOf(s)}>
          <input disabled={!lock} value={plotSet[`min${k}`]} onChange={(e) => setPlotSet((p) => ({ ...p, [`min${k}`]: e.target.value }))} />
        </Row>
        <Row label="Max:" unit={unitsOf(s)}>
          <input disabled={!lock} value={plotSet[`max${k}`]} onChange={(e) => setPlotSet((p) => ({ ...p, [`max${k}`]: e.target.value }))} />
        </Row>
      </div>
    );
  };

  return (
    <Shell title={title} width={560} onCancel={() => setSimDialog(null)} onOk={ok}>
      {plot && (
        <div className="sim-pages" role="tablist">
          <button type="button" role="tab" aria-selected={page === "command"} className={page === "command" ? "active" : ""} onClick={() => setPage("command")}>
            SPICE Command
          </button>
          <button type="button" role="tab" aria-selected={page === "plot"} className={page === "plot" ? "active" : ""} onClick={() => setPage("plot")}>
            Plot Setup
          </button>
        </div>
      )}
      {page === "command" ? (
        <>
          {d.mode === "new" && (
            <Row label="Analysis type:">
              <select aria-label="Analysis type" value={f.page} onChange={(e) => set({ page: e.target.value as SimType })}>
                {NEW_TAB_TYPES.map((t) => (
                  <option key={t} value={t}>
                    {shortName(t)}  {"—"}  {longName(t)}
                  </option>
                ))}
              </select>
            </Row>
          )}
          <div className="sim-page">{pages[f.page]}</div>
          <div className="sim-options">
            <label className="sim-check">
              <input type="checkbox" checked={opts.fixIncludePaths} onChange={(e) => setOpts((o) => ({ ...o, fixIncludePaths: e.target.checked }))} />
              Add full path for .include library directives
            </label>
            <label className="sim-check">
              <input type="checkbox" checked={opts.saveAllVoltages} onChange={(e) => setOpts((o) => ({ ...o, saveAllVoltages: e.target.checked }))} />
              Save all voltages
            </label>
            <label className="sim-check">
              <input type="checkbox" checked={opts.saveAllCurrents} onChange={(e) => setOpts((o) => ({ ...o, saveAllCurrents: e.target.checked }))} />
              Save all currents
            </label>
            <label className="sim-check">
              <input type="checkbox" checked={opts.saveAllDissipations} onChange={(e) => setOpts((o) => ({ ...o, saveAllDissipations: e.target.checked }))} />
              Save all power dissipations
            </label>
            <label className="sim-check">
              <input type="checkbox" checked={opts.saveAllEvents} onChange={(e) => setOpts((o) => ({ ...o, saveAllEvents: e.target.checked }))} />
              Save all digital event data
            </label>
            <Row label="Compatibility mode:">
              <select aria-label="Compatibility mode" value={mode} onChange={(e) => setMode(Number(e.target.value))}>
                {MODES.map((m, i) => (
                  <option key={m} value={i}>
                    {m}
                  </option>
                ))}
              </select>
            </Row>
          </div>
        </>
      ) : plot ? (
        <div className="sim-page">
          {lockRow(1, plot.y1)}
          {lockRow(2, plot.y2)}
          {lockRow(3, plot.y3)}
          <label className="sim-check">
            <input type="checkbox" checked={plotSet.grid} onChange={(e) => setPlotSet((p) => ({ ...p, grid: e.target.checked }))} />
            Show grid
          </label>
          <label className="sim-check">
            <input type="checkbox" checked={plotSet.legend} onChange={(e) => setPlotSet((p) => ({ ...p, legend: e.target.checked }))} />
            Show legend
          </label>
          <label className="sim-check">
            <input type="checkbox" checked={plotSet.dotted} onChange={(e) => setPlotSet((p) => ({ ...p, dotted: e.target.checked }))} />
            Dotted current/phase
          </label>
          <div>Margins:</div>
          {(["left", "top", "bottom", "right"] as const).map((k) => (
            <Row key={k} label={`${k.charAt(0).toUpperCase()}${k.slice(1)}:`}>
              <input value={plotSet[k]} onChange={(e) => setPlotSet((p) => ({ ...p, [k]: e.target.value }))} />
            </Row>
          ))}
        </div>
      ) : null}
      {error && (
        <div className="run-error" role="alert">
          {error}
        </div>
      )}
    </Shell>
  );
}

/** `DIALOG_USER_DEFINED_SIGNALS`: a row per signal (its id kept), Add takes the next free id, Delete takes the row. */
function UserSignalsDialog() {
  const ui = useSimUi();
  const [rows, setRows] = useState(() => [...ui.frame.userSignals.entries()].map(([id, s]) => ({ id, s })));
  const [help, setHelp] = useState(false);
  const ok = () => {
    const m = new Map<number, string>();
    for (const r of rows) if (r.s !== "") m.set(r.id, r.s);
    setUserSignals(getSimUi().frame, m);
    setSimDialog(null);
    void rerunIfFinished();
  };
  return (
    <Shell title="User-defined Signals" width={480} onCancel={() => setSimDialog(null)} onOk={ok}>
      <table className="sim-grid" aria-label="User-defined signals">
        <tbody>
          {rows.map((r, i) => (
            <tr key={r.id}>
              <td style={{ width: "100%" }}>
                <input className="sim-cell" aria-label={`Signal ${i + 1}`} value={r.s} onChange={(e) => setRows((rs) => rs.map((x, j) => (j === i ? { ...x, s: e.target.value } : x)))} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="sim-buttons">
        <button type="button" aria-label="Add signal" onClick={() => setRows((rs) => [...rs, { id: rs.reduce((n, x) => Math.max(n, x.id + 1), 0), s: "" }])}>
          +
        </button>
        <button type="button" aria-label="Delete signal" onClick={() => setRows((rs) => rs.slice(0, -1))}>
          −
        </button>
        <button type="button" className="link" onClick={() => setHelp(!help)}>
          Syntax help
        </button>
      </div>
      {help && (
        <div className="sim-help">
          Expressions of the signals and the functions ngspice knows: V(net), I(device), sqrt(x), sin(x), cos(x), tan(x), exp(x), ln(x), log(x), abs(x), pow(x,y), min(x,y), max(x,y) ... (KiCad's
          user_defined_signals_help.md).
        </div>
      )}
    </Shell>
  );
}

/** `NETLIST_VIEW_DIALOG`: the netlist, read only, with line numbers, and what the netlister said under it. */
function NetlistDialog({ text, messages }: { text: string; messages: string[] }) {
  const lines = text.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  return (
    <Shell title="SPICE Netlist" width={800} onCancel={() => setSimDialog(null)}>
      <pre className="sim-netlist" aria-label="SPICE netlist">
        {lines.map((l, i) => (
          <div key={i}>
            <span className="sim-lineno">{i + 1}</span>
            {l}
          </div>
        ))}
      </pre>
      {messages.length > 0 && <div className="sim-messages">{messages.join("\n")}</div>}
    </Shell>
  );
}

/** `DIALOG_SIM_FORMAT_VALUE`: significant digits (1 to 9) and a range, Auto or a prefix. */
function FormatDialog({ title, format, apply }: { title: string; format: SpiceValueFormat; apply: (f: SpiceValueFormat) => void }) {
  const r = format.range;
  const units = r.endsWith("Hz") ? "Hz" : r.endsWith("dB") ? "dB" : r.startsWith("~") ? r.slice(1) : /^[fpnumKMGTP]/.test(r) && r.length > 1 ? r.slice(1) : r;
  const [precision, setPrecision] = useState(format.precision);
  const [range, setRange] = useState(r.startsWith("~") ? "~" : r);
  const prefixes = ["f", "p", "n", "u", "m", "", "K", "M", "G", "T", "P"];
  return (
    <Shell title={`Format ${title}`} width={320} onCancel={() => setSimDialog(null)} onOk={() => {
      apply({ precision: Math.max(1, Math.min(9, precision)), range: range === "~" ? `~${units}` : range });
      setSimDialog(null);
    }}>
      <Row label="Significant digits:">
        <input type="number" min={1} max={9} value={precision} onChange={(e) => setPrecision(Number(e.target.value))} />
      </Row>
      <Row label="Range:">
        <select value={range} onChange={(e) => setRange(e.target.value)}>
          <option value="~">Auto</option>
          {prefixes.map((p) => (
            <option key={p} value={`${p}${units}`}>
              {p}
              {units}
            </option>
          ))}
        </select>
      </Row>
      <div className="sim-hint">{formatToString({ precision, range: range === "~" ? `~${units}` : range })}</div>
    </Shell>
  );
}

function FourierDialog({ signals, title }: { signals: string[]; title: string }) {
  const [v, setV] = useState("1K");
  return (
    <Shell title={title} width={360} onCancel={() => setSimDialog(null)} onOk={() => void doFourier(signals, v === "" ? "1K" : v)}>
      <Row label="Fundamental frequency:">
        <input aria-label="Fundamental frequency" value={v} onChange={(e) => setV(e.target.value)} />
      </Row>
    </Shell>
  );
}

function SaveAsDialog({ name, download }: { name: string; download: boolean }) {
  const [v, setV] = useState(name);
  return (
    <Shell title="Save Simulation Workbook As" width={420} onCancel={() => setSimDialog(null)} onOk={() => void saveWorkbookAs(v, download)} okLabel="Save">
      <Row label="File name:">
        <input aria-label="Workbook file name" value={v} onChange={(e) => setV(e.target.value)} />
      </Row>
      <div className="sim-hint">The workbook is kept in the design as the project's workbook file (the derived project's {v.endsWith(".wbk") ? v : `${v}.wbk`}){download ? ", and downloaded" : ""}.</div>
    </Shell>
  );
}

export function SimDialogs() {
  const ui = useSimUi();
  const d = ui.dialog;
  if (!d) return null;
  switch (d.kind) {
    case "command":
      return <CommandDialog d={d} />;
    case "userSignals":
      return <UserSignalsDialog />;
    case "netlist":
      return <NetlistDialog text={d.text} messages={d.messages} />;
    case "format":
      return <FormatDialog title={d.title} format={d.format} apply={d.apply} />;
    case "fourier":
      return <FourierDialog signals={d.signals} title={d.title} />;
    case "saveAs":
      return <SaveAsDialog name={d.name} download={d.download} />;
    case "confirm":
      return (
        <div className="dialog-backdrop sim-dialog">
          <div className="dialog" role="dialog" aria-label={d.title} style={{ width: 420 }}>
            <div className="dialog-header">{d.title}</div>
            <div className="dialog-body">{d.message}</div>
            <div className="dialog-footer">
              {d.buttons.map((b) => (
                <button key={b} type="button" onClick={() => d.resolve(b)}>
                  {b}
                </button>
              ))}
            </div>
          </div>
        </div>
      );
    case "message":
      return (
        <Shell title={d.title} width={480} onCancel={() => setSimDialog(null)}>
          <div role={d.error ? "alert" : "status"}>{d.message}</div>
          {d.detail && <pre className="sim-messages">{d.detail}</pre>}
        </Shell>
      );
  }
  void touch;
  return null;
}
