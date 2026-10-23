// The simulator's side panel (`SIMULATOR_FRAME_UI_BASE::m_sidePanel`): the signals grid with its filter, the cursors grid, the measurements grid and the
// tuners (simulator_frame_ui.cpp: rebuildSignalsGrid, onSignalsGridCellChanged, SIGNALS_GRID_TRICKS, updatePlotCursors, onCursorsGridCellChanged,
// CURSORS_GRID_TRICKS, rebuildMeasurementsGrid, onMeasurementsGridCellChanged, MEASUREMENTS_GRID_TRICKS; eeschema/widgets/tuner_slider.cpp).
import { useState } from "react";
import { cssRgb, parseColor } from "../../kicad-port/sim/mathPlot";
import { addMeasurement, createCursor, currentTab, cursorColumns, cursorRows, deleteCursor, deleteMeasurement, findTrace, setCursor, setCursorX, setMeasurement, setSignalPlotted, setTunerRange, setTunerSlider, setTunerValue, simTypeOf, sliderOf } from "../../kicad-port/sim/simFrame";
import { measurementFor, measurementValue, signalsGrid } from "../../kicad-port/sim/simSignals";
import { setTraceColor } from "../../kicad-port/sim/simPlotTab";
import { SpiceValue, formatFromString, formatToString } from "../../kicad-port/sim/spiceValue";
import { getSimUi, rerunIfFinished, removeTuner, saveTunerValue, setSimDialog, touch, tunerChanged, useSimUi } from "../../state/simulator";
import { useStudioState } from "../../state/store";
import type { Tuner } from "../../kicad-port/sim/simFrame";

/** A grid cell typed into: committed by Enter or by leaving it, when it changed (a `wxGrid` cell editor's `CELL_CHANGED`). */
function CommitInput({ value, label, disabled, onCommit }: { value: string; label: string; disabled?: boolean; onCommit: (v: string) => void }) {
  const [text, setText] = useState(value);
  const [shown, setShown] = useState(value);
  if (shown !== value) {
    setShown(value);
    setText(value);
  }
  const commit = () => {
    if (text !== value) onCommit(text);
  };
  return (
    <input
      className="sim-cell"
      aria-label={label}
      disabled={disabled}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
      }}
      onBlur={commit}
    />
  );
}

type Menu = { x: number; y: number; items: { label: string; fn?: () => void }[] } | null;

function ContextMenu({ menu, close }: { menu: Menu; close: () => void }) {
  if (!menu) return null;
  return (
    <div className="menubar-dropdown sim-context" style={{ left: menu.x, top: menu.y }} onMouseLeave={close}>
      {menu.items.map((it, i) =>
        it.fn ? (
          <div key={i} className="menu-node-item" role="menuitem" onClick={() => {
            close();
            it.fn!();
          }}>
            <span>{it.label}</span>
          </div>
        ) : (
          <div key={i} className="menu-separator" role="separator" />
        )
      )}
    </div>
  );
}

/** `SIGNALS_GRID_TRICKS::showPopupMenu` on a signal: the measurements for a TRAN, AC, DC or SP tab, Fourier for TRAN, Copy, Create new cursor. */
function signalMenu(signals: string[], x: number, y: number): Menu {
  const tab = currentTab(getSimUi().frame);
  const t = simTypeOf(tab);
  if (!(t === "TRAN" || t === "AC" || t === "DC" || t === "SP")) return null;
  const measure = (func: string) => () => {
    const f = getSimUi().frame;
    let added = false;
    for (const s of signals) {
      const m = measurementFor(func, s);
      if (m && addMeasurement(f, m)) added = true;
    }
    if (added) void rerunIfFinished();
  };
  const freq = t === "AC" || t === "SP";
  const items: NonNullable<Menu>["items"] = [
    { label: "Measure Min", fn: measure("MIN") },
    { label: "Measure Max", fn: measure("MAX") },
    { label: "Measure Average", fn: measure("AVG") },
    { label: "Measure RMS", fn: measure("RMS") },
    { label: "Measure Peak-to-peak", fn: measure("PP") },
    { label: freq ? "Measure Frequency of Min" : "Measure Time of Min", fn: measure("MIN_AT") },
    { label: freq ? "Measure Frequency of Max" : "Measure Time of Max", fn: measure("MAX_AT") },
    { label: "Measure Integral", fn: measure("INTEG") },
  ];
  if (t === "TRAN") {
    items.push({ label: "" }, { label: "Perform Fourier Analysis...", fn: () => setSimDialog({ kind: "fourier", signals, title: signals.length === 1 ? `Fourier Analysis of ${signals[0]}` : "Fourier Analyses of Multiple Signals" }) });
  }
  items.push({ label: "" }, { label: "Copy Signal Name", fn: () => void navigator.clipboard?.writeText(signals.join("\r")).catch(() => undefined) });
  items.push({ label: "" }, { label: "Create new cursor...", fn: () => {
    createCursor(getSimUi().frame);
    touch();
  } });
  return { x, y, items };
}

function SignalsGrid() {
  const ui = useSimUi();
  const f = ui.frame;
  const tab = currentTab(f);
  const [menu, setMenu] = useState<Menu>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  if (!tab?.plot) return <div className="sim-grid-empty" />;
  const list = signalsGrid(simTypeOf(tab), tab.command, f.signals, f.userSignals, f.filter);
  const cols = cursorColumns(f);
  return (
    <div className="sim-section">
      <input className="sim-filter" type="search" placeholder="Filter" aria-label="Filter" value={f.filter} onChange={(e) => {
        f.filter = e.target.value;
        touch();
      }} />
      <div className="sim-grid-wrap">
        <table className="sim-grid" aria-label="Signals">
          <thead>
            <tr>
              <th>Signal</th>
              <th>Plot</th>
              <th>Color</th>
              {cols.map((c) => (
                <th key={c} onContextMenu={(e) => {
                  e.preventDefault();
                  const items: NonNullable<Menu>["items"] = [{ label: "Create new cursor...", fn: () => (createCursor(f), touch()) }];
                  if (c > 2 && c === cols[cols.length - 1]) items.push({ label: "" }, { label: `Delete Cursor ${c}...`, fn: () => (deleteCursor(f), touch()) });
                  setMenu({ x: e.clientX, y: e.clientY, items });
                }}>
                  Cursor {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {list.map((signal) => {
              const tr = findTrace(f, tab, signal);
              return (
                <tr key={signal} className={selected.has(signal) ? "selected" : ""}>
                  <td
                    className="sim-signal"
                    onClick={(e) => setSelected((s) => (e.ctrlKey || e.metaKey ? new Set([...s, signal]) : new Set([signal])))}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      const sigs = selected.has(signal) ? [...selected] : [signal];
                      setMenu(signalMenu(sigs, e.clientX, e.clientY));
                    }}
                  >
                    {signal}
                  </td>
                  <td>
                    <input type="checkbox" aria-label={`Plot ${signal}`} checked={!!tr} onChange={(e) => {
                      setSignalPlotted(f, signal, e.target.checked);
                      touch();
                    }} />
                  </td>
                  <td>
                    {tr && (
                      <input type="color" aria-label={`Color of ${signal}`} value={`#${tr.color.map((c) => Math.round(c).toString(16).padStart(2, "0")).join("")}`} onChange={(e) => {
                        const c = parseColor(e.target.value);
                        if (c && tab.plot) {
                          setTraceColor(tab.plot, tr, c);
                          touch(true);
                        }
                      }} style={{ background: cssRgb(tr.color) }} />
                    )}
                  </td>
                  {cols.map((c) => (
                    <td key={c}>
                      {tr && (
                        <input type="checkbox" aria-label={`Cursor ${c} on ${signal}`} checked={tr.cursors.has(c)} onChange={(e) => {
                          setCursor(f, signal, c, e.target.checked);
                          touch();
                        }} />
                      )}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <ContextMenu menu={menu} close={() => setMenu(null)} />
    </div>
  );
}

function CursorsGrid() {
  const ui = useSimUi();
  const f = ui.frame;
  const [menu, setMenu] = useState<Menu>(null);
  const { rows, xLabel, yLabel } = cursorRows(f);
  // `CURSORS_GRID_TRICKS`: "Format <column>..." on the x and y columns, the format then given to every row of the same signal.
  const formatMenu = (rowIndex: number, axis: 0 | 1, e: React.MouseEvent) => {
    e.preventDefault();
    const row = rows[rowIndex];
    if (!row) return;
    const idx = row.id === null ? 2 : row.id === 1 ? 0 : row.id === 2 ? 1 : row.id;
    const label = axis === 0 ? xLabel : yLabel;
    setMenu({ x: e.clientX, y: e.clientY, items: [{ label: `Format ${label}...`, fn: () => setSimDialog({ kind: "format", title: label, format: { ...f.cursorFormats[idx]![axis]! }, apply: (fm) => {
      const name = (r: { signal: string }) => (r.signal.endsWith("[2 - 1]") ? r.signal.slice(0, -7) : r.signal);
      rows.forEach((r) => {
        if (name(r) !== name(row)) return;
        const i = r.id === null ? 2 : r.id === 1 ? 0 : r.id === 2 ? 1 : r.id;
        f.cursorFormats[i]![axis] = { ...fm };
      });
      touch(true);
    } }) }] });
  };
  return (
    <div className="sim-section">
      <table className="sim-grid" aria-label="Cursors">
        <thead>
          <tr>
            <th>Cursor</th>
            <th>Signal</th>
            <th>{xLabel}</th>
            <th>{yLabel}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={`${r.name}-${i}`}>
              <td>{r.name}</td>
              <td>{r.signal}</td>
              <td onContextMenu={(e) => formatMenu(i, 0, e)}>
                <CommitInput label={`Cursor ${r.name} ${xLabel}`} value={r.x} onCommit={(v) => {
                  setCursorX(f, r, v);
                  touch();
                }} />
              </td>
              <td onContextMenu={(e) => formatMenu(i, 1, e)}>{r.y}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <ContextMenu menu={menu} close={() => setMenu(null)} />
    </div>
  );
}

function MeasurementsGrid() {
  const ui = useSimUi();
  const f = ui.frame;
  const tab = currentTab(f);
  const [menu, setMenu] = useState<Menu>(null);
  const t = simTypeOf(tab);
  if (!tab?.plot) return <div className="sim-section" />;
  const ms = tab.plot.measurements;
  // `rebuildMeasurementsGrid`: an empty row to type into for TRAN, AC, DC and SP.
  const typeIn = t === "TRAN" || t === "AC" || t === "DC" || t === "SP";
  const rows = typeIn ? [...ms, { expr: "", format: "" }] : ms;
  return (
    <div className="sim-section">
      <table className="sim-grid" aria-label="Measurements">
        <thead>
          <tr>
            <th>Measurement</th>
            <th>Value</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((m, row) => (
            <tr key={`${row}-${m.expr}`} onContextMenu={(e) => {
              e.preventDefault();
              const items: NonNullable<Menu>["items"] = [];
              if (row < ms.length) {
                items.push({ label: "Format Value...", fn: () => setSimDialog({ kind: "format", title: "Value", format: formatFromString(m.format), apply: (fm) => {
                  m.format = formatToString(fm);
                  touch(true);
                } }) });
                items.push({ label: "Delete Measurement", fn: () => {
                  deleteMeasurement(f, row);
                  void rerunIfFinished();
                } });
              }
              setMenu({ x: e.clientX, y: e.clientY, items });
            }}>
              <td>
                <CommitInput label={`Measurement ${row + 1}`} value={m.expr} onCommit={(v) => {
                  setMeasurement(f, row, v);
                  void rerunIfFinished();
                }} />
              </td>
              <td>{row < ms.length ? (f.finished || tab.measured.size > 0 ? measurementValue(tab.measured.get(row), m.format) : "?") : ""}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <ContextMenu menu={menu} close={() => setMenu(null)} />
    </div>
  );
}

/** `TUNER_SLIDER`: "Tune R1", the range, the slider and the value, the E series, Save and remove. */
function TunerView({ t }: { t: Tuner }) {
  const state = useStudioState();
  const fieldsOf = (ref: string): Record<string, string> | null => {
    const sym = state.schematic?.symbols.find((s) => s.id === ref) as unknown as { fields?: { name: string; text: string }[] } | undefined;
    if (!sym?.fields) return null;
    return Object.fromEntries(sym.fields.map((x) => [x.name, x.text]));
  };
  const range = (which: "min" | "max", text: string) => {
    const v = SpiceValue.parse(text);
    if (v && setTunerRange(t, which, v)) {
      touch(true);
      if (t.multi) void tunerChanged();
    } else touch();
  };
  return (
    <div className="sim-tuner" aria-label={`Tune ${t.ref}`}>
      <div className="sim-tuner-head">
        <span>Tune {t.ref}</span>
        <button type="button" aria-label={`Remove tuner ${t.ref}`} onClick={() => removeTuner(t)}>
          ×
        </button>
      </div>
      <CommitInput label={`${t.ref} maximum`} value={t.max.toOrigString()} onCommit={(v) => range("max", v)} />
      <input type="range" min={0} max={100} aria-label={`${t.ref} value slider`} disabled={t.multi} value={sliderOf(t)} onChange={(e) => {
        setTunerSlider(t, Number(e.target.value));
        touch(true);
      }} onPointerUp={() => void tunerChanged()} onKeyUp={() => void tunerChanged()} />
      <CommitInput label={`${t.ref} minimum`} value={t.min.toOrigString()} onCommit={(v) => range("min", v)} />
      <CommitInput label={`${t.ref} value`} disabled={t.multi} value={t.text} onCommit={(text) => {
        const v = SpiceValue.parse(text);
        if (v) {
          setTunerValue(t, v);
          touch(true);
          void tunerChanged();
        } else touch();
      }} />
      <div className="sim-tuner-series">
        {(["E24", "E48", "E96", "E192"] as const).map((s) => (
          <label key={s}>
            <input type="checkbox" checked={t.series === s} onChange={(e) => {
              t.series = e.target.checked ? s : "";
              const before = t.text;
              setTunerValue(t, t.value);
              touch(true);
              if (t.text !== before) void tunerChanged();
            }} />
            {s}
          </label>
        ))}
      </div>
      <div className="sim-tuner-mode">
        <select aria-label={`${t.ref} run mode`} value={t.multi ? "multi" : "single"} onChange={(e) => {
          t.multi = e.target.value === "multi";
          touch(true);
          void tunerChanged();
        }}>
          <option value="single">Single Run</option>
          <option value="multi">Multi Run</option>
        </select>
        <label>
          Steps
          <input type="number" min={2} max={100} disabled={!t.multi} value={t.steps} onChange={(e) => {
            t.steps = Math.max(2, Number(e.target.value) || 2);
            touch(true);
          }} onBlur={() => void tunerChanged()} />
        </label>
        <button type="button" disabled={t.multi} onClick={() => void saveTunerValue(t, fieldsOf(t.ref))}>
          Save
        </button>
      </div>
    </div>
  );
}

export function SimSidePanel() {
  const ui = useSimUi();
  return (
    <div className="sim-side" aria-label="Simulation side panel">
      <SignalsGrid />
      <CursorsGrid />
      <MeasurementsGrid />
      <div className="sim-tuners">
        {ui.frame.tuners.map((t) => (
          <TunerView key={t.ref} t={t} />
        ))}
      </div>
    </div>
  );
}
