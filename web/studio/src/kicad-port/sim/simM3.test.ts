// The simulator's details of round 3 (UI-LEFT-3 M3-3) against KiCad 8303b2ad's C++: the measurements grid's rows (`onMeasurementsGridCellChanged`,
// `updateMeasurementsFromGrid`, `rebuildMeasurementsGrid`), an empty trace's bounds (`mpFXYVector::SetData`, `mpFXY::UpdateScales`), the probe on an
// S-parameter tab (`AddTrace`, `updateTrace`) and the points fields' `wxIntegerValidator` (wxWidgets 3.2: `IsCharOk`, `NormalizeString`, no message).
import { test } from "node:test";
import assert from "node:assert/strict";
import { addMeasurement, deleteMeasurement, measurementGridRows, measurementRequests, newFrame, newSimTab, rebuildMeasurementsGrid, setMeasurement } from "./simFrame";
import { getOrAddTrace, resetScales, setTraceData } from "./simPlotTab";
import { traceData, type RawPlot } from "./simSignals";
import { buildCommand, defaultFields, integerCharOk, normalizeInteger, SPT_AC_GAIN, SPT_AC_PHASE, SPT_SP_AMP, SPT_VOLTAGE } from "./simCommand";

test("a cleared measurement row stays empty until the grid is built again; the measurements are the rows with text", () => {
  const f = newFrame(true);
  const tab = newSimTab(f, ".tran 1u 1m");
  for (const m of ["MAX V(a)", "MIN V(a)", "AVG V(a)"]) addMeasurement(f, m);
  setMeasurement(f, 1, "");
  assert.deepEqual(measurementGridRows(tab).map((m) => m.expr), ["MAX V(a)", "", "AVG V(a)"], "the row stays, empty");
  assert.deepEqual(tab.plot!.measurements.map((m) => m.expr), ["MAX V(a)", "AVG V(a)"], "updateMeasurementsFromGrid");
  // the rows ask ngspice by their grid row (`meas_result_<row>`); an empty one asks nothing
  assert.deepEqual(measurementRequests(tab).map((r) => r.row), [0, 2]);
  // AddMeasurement fills the first empty row
  addMeasurement(f, "PP V(a)");
  assert.deepEqual(measurementGridRows(tab).map((m) => m.expr), ["MAX V(a)", "PP V(a)", "AVG V(a)"]);
  // trailing empty rows go ("Always leave a single empty row for type-in")
  setMeasurement(f, 2, "");
  assert.deepEqual(measurementGridRows(tab).map((m) => m.expr), ["MAX V(a)", "PP V(a)"]);
  setMeasurement(f, 0, "");
  deleteMeasurement(f, 1);
  assert.deepEqual(measurementGridRows(tab).map((m) => m.expr), [], "an empty row before the type-in row goes with it");
  for (const m of ["A V(a)", "B V(a)"]) addMeasurement(f, m);
  setMeasurement(f, 0, "");
  // a run, another tab, new settings: `rebuildMeasurementsGrid` from the tab's measurements
  rebuildMeasurementsGrid(tab);
  assert.deepEqual(measurementGridRows(tab).map((m) => m.expr), ["B V(a)"]);
});

test("an empty trace stretches the scales to 0, a trace with no data yet to -1 .. 1 (mpFXYVector::SetData, the constructor)", () => {
  const f = newFrame(true);
  const tab = newSimTab(f, ".tran 1u 1m");
  const plot = tab.plot!;
  const a = getOrAddTrace(plot, "V(a)", SPT_VOLTAGE, true);
  setTraceData(plot, a, [0.1e-3, 0.5e-3], [5, 10], 1, Number.POSITIVE_INFINITY);
  resetScales(plot, false);
  assert.deepEqual([plot.y1!.minV, plot.y1!.maxV], [5, 10]);
  const b = getOrAddTrace(plot, "V(b)", SPT_VOLTAGE, true);
  resetScales(plot, false);
  assert.deepEqual([plot.y1!.minV, plot.y1!.maxV], [-1, 10], "no data yet: the constructor's -1 .. 1");
  setTraceData(plot, b, [], [], 1, Number.POSITIVE_INFINITY);
  resetScales(plot, false);
  assert.deepEqual([plot.y1!.minV, plot.y1!.maxV], [0, 10], "empty vectors: (0, 0)");
});

test("a probe on an S-parameter tab: the gain half asked with SPT_AC_GAIN gets no data, the phase half its phase (updateTrace)", () => {
  const plot: RawPlot = { plot_name: "sp1", complex: true, points: 2, vectors: [{ name: "frequency", kind: "frequency", re: [1e3, 1e4] }, { name: "v(out)", kind: "voltage", re: [3, 0], im: [4, 1] }] };
  assert.equal(traceData("SP", ".sp dec 10 1k 10k 1", plot, "V(out)", SPT_VOLTAGE | SPT_AC_GAIN), null, "`wxFAIL_MSG`: no SPT_SP_AMP");
  assert.deepEqual(traceData("SP", ".sp dec 10 1k 10k 1", plot, "V(out)", SPT_VOLTAGE | SPT_SP_AMP)?.y, [5, 1]);
  assert.deepEqual(traceData("SP", ".sp dec 10 1k 10k 1", plot, "V(out)", SPT_VOLTAGE | SPT_AC_PHASE)?.y.map((p) => Math.round(p * 1000) / 1000), [0.927, 1.571]);
});

test("the points fields take digits only and are normalized on leaving them; OK writes them as they are (wxWidgets 3.2's wxIntegerValidator)", () => {
  assert.ok(integerCharOk("7") && !integerCharOk("e") && !integerCharOk(".") && !integerCharOk("-"), "IsCharOk; no minus sign for a minimum of 1");
  assert.ok(integerCharOk("-", -5));
  assert.deepEqual(["007", "10", "", "1e2", "x", " 12 ", "0"].map(normalizeInteger), ["7", "10", "", "", "", "12", "0"]);
  const ac = { ...defaultFields(), page: "AC" as const, acPoints: "", acStart: "1", acStop: "1Meg" };
  assert.deepEqual(buildCommand(ac, (r) => r), { command: ".ac dec  1 1Meg" }, "no message: Validate() is true");
});
