import { test } from "node:test";
import assert from "node:assert/strict";
import { SpiceValue, formatFromString, formatG, formatToString, formatWithUnits } from "./spiceValue";
import { OPTION_DEFAULT_FLAGS, OPTION_ADJUST_PASSIVE_VALS, OPTION_SAVE_ALL_EVENTS, buildCommand, commandFromCommands, commandToSimType, commandsOf, defaultFields, parseCommand, parseDcCommand, parseNoiseCommand, SPT_AC_GAIN, SPT_AC_PHASE, SPT_CURRENT, SPT_POWER, SPT_VOLTAGE, SPT_TIME } from "./simCommand";
import { countDecimalDigits, fit, fitTo, newScale, newView, siSuffix, valueAt, xTicks, yTicks, zoomIn, zoomOut, zoomRect, zoomUndo, zoomRedo, toPlot, extendDataRange, generateColor, colorList } from "./mathPlot";
import { findVector, measurementCommand, measurementFor, opResults, plotCsv, signalMatches, signalsList, sortSignals, traceData, vectorOfSignal, vectorToSignal, type Circuit, type RawPlot } from "./simSignals";
import { addTrace, applyRun, cursorRows, createCursor, fromWorkbook, multiRunSteps, newFrame, newSimTab, newTuner, setCursor, setSignalPlotted, setTunerSlider, setUserSignals, toWorkbook, tunerText } from "./simFrame";

test("SPICE values read and write as KiCad's SPICE_VALUE does", () => {
  assert.equal(SpiceValue.parse("10k")!.toDouble(), 10_000);
  assert.equal(SpiceValue.parse("1Meg")!.toSpiceString(), "1Meg");
  assert.equal(SpiceValue.parse("1500000")!.toSpiceString(), "1.5Meg");
  assert.equal(SpiceValue.parse("0.001")!.toSpiceString(), "1m");
  assert.equal(SpiceValue.parse("abc"), null);
  assert.equal(SpiceValue.of(1 / 3).toSpiceString(), "333.333m");
  assert.equal(SpiceValue.of(4.99326).toFormatted(formatFromString("3~V")), "4.99V");
  assert.equal(SpiceValue.of(0.0031623).toFormatted(formatFromString("3~V")), "3.16mV");
  assert.equal(SpiceValue.of(12345).toFormatted(formatFromString("3~V")), "12.3KV");
  assert.equal(SpiceValue.of(1.5).toFormatted(formatFromString("4mV")), "1500mV");
  assert.equal(SpiceValue.of(-190).toFormatted(formatFromString("1°")), "170.0°");
  assert.equal(formatToString(formatWithUnits(formatFromString("2mV"), "s")), "2ms");
  assert.equal(formatG(1e-5), "1e-05");
  assert.equal(formatG(1234567), "1.23457e+06");
  assert.equal(SpiceValue.of(0.5).mul(SpiceValue.parse("10k")!).toSpiceString(), "5k");
});

test("commands are typed and read field by field", () => {
  assert.equal(commandToSimType(".tran 1u 1m"), "TRAN");
  assert.equal(commandToSimType(".op"), "OP");
  assert.equal(commandToSimType("linearize v(out)\nfft v(out)"), "FFT");
  const dc = parseDcCommand(".dc V1 0 5 0.1 V2 0 1 0.5")!;
  assert.equal(dc.src1.source, "V1");
  assert.equal(dc.src2!.incr.toDouble(), 0.5);
  const n = parseNoiseCommand(".noise v(out,in) V1 dec 10 1 1Meg 1")!;
  assert.equal(n.ref, "in");
  assert.equal(n.fStop.toDouble(), 1e6);
  assert.equal(n.saveAll, true);
});

test("the analysis dialog makes the commands KiCad's makes", () => {
  const id = (r: string) => r;
  const ok = (r: { command: string } | { error: string }) => {
    assert.ok("command" in r, JSON.stringify(r));
    return (r as { command: string }).command;
  };
  const tran = { ...defaultFields(), page: "TRAN" as const, tranStep: "10u", tranFinal: "5m" };
  assert.equal(ok(buildCommand(tran, id)), ".tran 10u 5m");
  // "use initial conditions" with no max step: the step is min(tstep, (tstop - tstart) / 50), and the start 0
  assert.equal(ok(buildCommand({ ...tran, tranUic: true }, id)), ".tran 10u 5m 0 10u uic");
  assert.equal(ok(buildCommand({ ...tran, tranStep: "1m", tranUic: true }, id)), ".tran 1m 5m 0 100u uic");
  assert.equal(ok(buildCommand({ ...tran, tranInitial: "1m", tranMaxStep: "2u" }, id)), ".tran 10u 5m 1m 2u");
  const ac = { ...defaultFields(), page: "AC" as const, acPoints: "10", acStart: "1", acStop: "1Meg" };
  assert.equal(ok(buildCommand(ac, id)), ".ac dec 10 1 1Meg");
  const dc = { ...defaultFields(), page: "DC" as const, dcSource1: "V1", dcStart1: "0", dcStop1: "5", dcIncr1: "0.1" };
  assert.equal(ok(buildCommand(dc, id)), ".dc V1 0 5 100m");
  assert.deepEqual(buildCommand({ ...dc, dcSource1: "" }, id), { error: "A DC source must be specified." });
  assert.deepEqual(buildCommand({ ...dc, dcEnable2: true, dcSource2: "V1", dcStart2: "0", dcStop2: "1", dcIncr2: "1" }, id), { error: "Source 1 and Source 2 must be different." });
  assert.deepEqual(buildCommand({ ...defaultFields(), page: "NOISE" as const }, id), { error: "A frequency range must be specified." });
  const noise = { ...defaultFields(), page: "NOISE" as const, noiseMeas: "/out", noiseRef: "/in", noiseSrc: "V1", noisePoints: "10", noiseStart: "1", noiseStop: "10k", noiseSaveAll: false };
  assert.equal(ok(buildCommand(noise, id)), ".noise v(/out,/in) V1 dec 10 1 10k");
  assert.equal(ok(buildCommand({ ...defaultFields(), page: "OP" as const }, id)), ".op");
  assert.equal(ok(buildCommand({ ...defaultFields(), page: "FFT" as const, fftSignals: ["V(/out)"] }, id)), "linearize V(/out)\nfft V(/out)");
  // and back
  const f = parseCommand(".tran 10u 5m 0 10u uic");
  assert.equal(f.page, "TRAN");
  assert.equal(f.tranMaxStep, "10u");
  assert.equal(f.tranUic, true);
  assert.equal(parseCommand(".ac oct 5 10 1k").acScale, "oct");
  assert.equal(parseCommand("* my own", "").custom, "* my own");
});

test("a workbook's commands carry the options", () => {
  const c = commandsOf(".op", OPTION_DEFAULT_FLAGS);
  assert.deepEqual(c, [".op", ".kicad adjustpaths", ".save all", ".probe alli", ".probe allp"]);
  assert.deepEqual(commandFromCommands(c), { command: ".op", options: OPTION_DEFAULT_FLAGS });
  assert.deepEqual(commandFromCommands([".ac dec 10 1 1k ", ".kicad esavenone"]), { command: ".ac dec 10 1 1k", options: OPTION_ADJUST_PASSIVE_VALS });
  assert.equal(commandFromCommands([".op"]).options & OPTION_SAVE_ALL_EVENTS, OPTION_SAVE_ALL_EVENTS);
});

test("mathplot's ticks, labels and suffixes", () => {
  assert.deepEqual(siSuffix(0.0045, "V"), { power: -3, suffix: "mV" });
  assert.deepEqual(siSuffix(5e12, "Hz"), { power: 0, suffix: "Hz" }, "the loop stops before tera");
  assert.equal(countDecimalDigits(2.5, 3), 1);
  assert.equal(countDecimalDigits(0.125, 3), 3);
  const v = newView(800, 400);
  const x = newScale("lin", "s", "bottom", "Time");
  extendDataRange(x, 0, 0.005);
  fit(v);
  xTicks(x, v);
  assert.ok(x.tickValues.length >= 4, `${x.tickValues}`);
  assert.equal(x.name, "Time (ms)");
  assert.ok(x.tickLabels.every((l) => /^\d+(\.\d+)?$/.test(l.label)), x.tickLabels.map((l) => l.label).join(","));
  const y = newScale("lin", "V", "left", "Voltage");
  extendDataRange(y, -1, 1);
  yTicks(y, v);
  assert.ok(y.tickValues.length >= 4 && y.tickLabels.some((l) => /^-?0\.0+$/.test(l.label)), y.tickLabels.map((l) => l.label).join(","));
  const lg = newScale("log", "Hz", "bottom", "Frequency");
  extendDataRange(lg, 1, 1e5);
  xTicks(lg, v);
  assert.ok(lg.tickLabels.some((l) => l.label === "1kHz"), lg.tickLabels.map((l) => l.label).join(","));
  assert.ok(Math.abs(toPlot(lg, 100) - 0.4) < 1e-12);
});

test("the view zooms, refuses to leave the data and keeps its history", () => {
  const v = newView(800, 400);
  fit(v);
  const before = [v.desiredXmin, v.desiredXmax];
  zoomIn(v, "h");
  assert.ok(v.desiredXmax - v.desiredXmin < before[1]! - before[0]!);
  assert.equal(v.undo.length, 1);
  zoomOut(v, "both");
  zoomOut(v, "both");
  assert.ok(v.desiredXmin >= 0 && v.desiredXmax <= 1 + 1e-12, "limited to the data");
  zoomUndo(v);
  zoomUndo(v);
  assert.equal(v.redo.length, 2);
  zoomRedo(v);
  assert.equal(v.redo.length, 1);
  zoomRect(v, { x: 100, y: 50 }, { x: 300, y: 300 });
  assert.equal(v.redo.length, 0, "a new zoom forgets the redo");
  fitTo(v, 0, 1, 0, 1);
  assert.equal(v.desiredXmin, 0);
});

test("a cursor's value is the trace's, interpolated", () => {
  assert.equal(valueAt([0, 1, 2], [0, 10, 20], 1.5), 15);
  assert.ok(Number.isNaN(valueAt([0, 1, 2], [0, 10, 20], 3)));
  assert.deepEqual(generateColor([], true), colorList(true)[3]);
  assert.deepEqual(generateColor([colorList(true)[3]!], true), colorList(true)[4]);
});

const circuit: Circuit = {
  nets: ["/in", "/out", "GND", "unconnected-_V3-Pad1_"],
  items: [
    { ref: "R1", name: "R1", device: "R", nodes: ["/in", "/out"], pin_count: 2, pin_index: { "1": 0, "2": 1 }, currents: ["I(R1)"], tuner: { param: "r", value: "1k" } },
    { ref: "V1", name: "V1", device: "V", nodes: ["/in", "GND"], pin_count: 2, pin_index: { "1": 0, "2": 1 }, currents: ["I(V1)"], tuner: { param: "dc", value: "5" } },
  ],
  pin_nets: { "R1.1": "/in" },
  directives: [".save v(/x)"],
  sch_sim_command: "",
  messages: [],
  ngspice: true,
};

test("the signals list is KiCad's for each analysis", () => {
  const tran = signalsList("TRAN", OPTION_DEFAULT_FLAGS, circuit).signals;
  assert.deepEqual(tran, ["V(/in)", "V(/out)", "I(R1)", "I(V1)", "P(R1)", "P(V1)", "V(/X)"]);
  const ac = signalsList("AC", OPTION_DEFAULT_FLAGS, circuit).signals;
  assert.ok(ac.includes("V(/out) (gain)") && ac.includes("I(R1) (phase)") && !ac.some((s) => s.startsWith("P(")));
  assert.deepEqual(signalsList("NOISE", OPTION_DEFAULT_FLAGS, circuit).signals.slice(0, 2), ["inoise_spectrum", "onoise_spectrum"]);
  assert.deepEqual(sortSignals(["I(R1)", "V(b10)", "V(b2)"]), ["V(b2)", "V(b10)", "I(R1)"]);
  assert.ok(signalMatches("", "V(x)") && signalMatches("v(*", "V(x)") && signalMatches("OUT", "V(/out)") && !signalMatches("zzz", "V(x)"));
  const users = new Map([[0, "V(/out)-V(/in)"]]);
  assert.deepEqual(vectorOfSignal("V(/out)-V(/in)", "TRAN", users), { vector: "user0", type: SPT_VOLTAGE });
  assert.deepEqual(vectorOfSignal("V(/out) (gain)", "AC", users), { vector: "V(/out)", type: SPT_VOLTAGE | SPT_AC_GAIN });
  assert.deepEqual(vectorOfSignal("V(/out)*I(R1)", "TRAN", new Map([[3, "V(/out)*I(R1)"]])), { vector: "user3", type: SPT_POWER });
});

const tranPlot: RawPlot = {
  plot_name: "Transient Analysis",
  complex: false,
  points: 3,
  vectors: [
    { name: "time", kind: "time", re: [0, 1e-3, 2e-3] },
    { name: "v(/out)", kind: "voltage", re: [0, 3.16, 4.32] },
    { name: "i(v1)", kind: "current", re: [-5e-3, -1.8e-3, -0.7e-3] },
    { name: "r1:power", kind: "notype", re: [0.025, 0.003, 0.0005] },
    { name: "meas_result_0", kind: "notype", re: [4.32, 0, 0] },
  ],
};

test("a run's vectors are found by the names KiCad asks for", () => {
  assert.equal(findVector(tranPlot, "V(/out)")!.name, "v(/out)");
  assert.equal(findVector(tranPlot, "I(V1)")!.name, "i(v1)");
  assert.equal(findVector(tranPlot, "v1#branch")!.name, "i(v1)");
  const d = traceData("TRAN", ".tran 1m 2m", tranPlot, "P(R1)", SPT_POWER)!;
  assert.deepEqual(d.y, [0.025, 0.003, 0.0005]);
  assert.equal(d.sweepCount, 1);
  assert.deepEqual(vectorToSignal("v1#branch"), { signal: "I(v1)", type: SPT_CURRENT });
  assert.deepEqual(vectorToSignal("v(/out)").signal, "V(/out)");
});

test("an operating point is written to the console and to the schematic", () => {
  const op: RawPlot = { plot_name: "Operating Point", complex: false, points: 1, vectors: [{ name: "v(/out)", kind: "voltage", re: [0.98946] }, { name: "i(v1)", kind: "current", re: [-1e-4] }, { name: "r1:power", kind: "notype", re: [2e-6] }] };
  const r = opResults(op);
  // `( signal + ":" ).Pad( 25 - signal.length )`: 25 - n spaces after the colon
  const row = (s: string, v: string) => `${s}:${" ".repeat(25 - s.length)}${v}\n`;
  assert.equal(r.console, `\n\nSimulation results:\n\n${row("V(/out)", "989.46mV")}${row("I(v1)", "-100uA")}${row("P(r1)", "2uW")}`);
  assert.equal(r.points.get("/out"), 0.98946);
  assert.equal(r.points.get("r1:power"), 2e-6);
});

test("measurements carry their units and a power is ngspice's", () => {
  assert.deepEqual(measurementCommand("MAX V(/out)", "TRAN", ""), { text: "MAX V(/out)", format: "3~V", units: "V" });
  assert.deepEqual(measurementCommand("AVG P(R1)", "TRAN", ""), { text: "AVG R1:power", format: "3~W", units: "W" });
  assert.equal(measurementCommand("MAX_AT V(/out)", "AC", "").units, "Hz");
  assert.equal(measurementCommand("INTEG I(R1)", "TRAN", "").units, "A.s");
  assert.equal(measurementCommand("INTEG P(R1)", "TRAN", "").units, "J");
  assert.equal(measurementFor("MAX", "V(/out) (gain)"), "MAX vdb(/out)");
  assert.equal(measurementFor("MAX", "V(/out) (phase)"), null);
});

test("CSV is KiCad's: ';' after every value, CRLF rows", () => {
  const csv = plotCsv("time", [{ id: "V(/out)1", display: "V(/out)", x: [0, 1e-3], y: [0, 3.16] }]);
  assert.equal(csv, "time;V(/out);\r\n0;0;\r\n0.001;3.16;\r\n");
  assert.equal(plotCsv("time", []), null);
});

test("a frame plots, runs, places cursors and makes the workbook KiCad writes", () => {
  const f = newFrame(true);
  f.circuit = circuit;
  const tab = newSimTab(f, ".tran 1m 2m");
  assert.equal(tab.title, "Analysis 1 - TRAN");
  setSignalPlotted(f, "V(/out)", true);
  addTrace(f, "I(V1)", SPT_CURRENT);
  assert.equal(tab.plot!.traces.size, 2);
  applyRun(f, tab, [tranPlot], [], "console");
  assert.equal(f.finished, true);
  assert.equal(tab.measured.get(0), 4.32);
  const v = [...tab.plot!.traces.values()].find((t) => t.vector === "V(/out)")!;
  assert.deepEqual(v.y, [0, 3.16, 4.32]);
  setCursor(f, "V(/out)", 1, true);
  const rows = cursorRows(f);
  assert.equal(rows.rows.length, 1);
  assert.equal(rows.rows[0]!.signal, "V(/out)");
  createCursor(f);
  assert.equal(f.customCursors, 4);
  setUserSignals(f, new Map([[0, "V(/out)*2"]]));
  const wb = toWorkbook(f);
  assert.equal(wb.version, 7);
  assert.equal(wb.custom_cursors, 3);
  assert.deepEqual(wb.user_defined_signals, ["V(/out)*2"]);
  const tabs = wb.tabs as Record<string, unknown>[];
  assert.equal(tabs[0]!.analysis, "TRAN");
  const traces = tabs[0]!.traces as Record<string, unknown>[];
  assert.equal(traces.length, 2);
  const vt = traces.find((t) => t.signal === "V(/out)")!;
  // a trace plotted before the first run keeps the type it was made with (KiCad's `GetOrAddTrace` finds it by its y bits); one made after has the x bits
  assert.equal(vt.trace_type, SPT_VOLTAGE);
  setSignalPlotted(f, "P(R1)", true);
  assert.equal([...tab.plot!.traces.values()].find((t) => t.vector === "P(R1)")!.type, SPT_POWER | SPT_TIME);
  setSignalPlotted(f, "P(R1)", false);
  assert.ok(vt.cursor1 && vt.cursorD, JSON.stringify(vt));
  // and back
  const g = newFrame(true);
  g.circuit = circuit;
  fromWorkbook(g, JSON.parse(JSON.stringify(wb)));
  assert.equal(g.tabs.length, 1);
  assert.equal(g.customCursors, 4);
  assert.equal(g.tabs[0]!.plot!.traces.size, 2);
  assert.equal([...g.tabs[0]!.plot!.traces.values()].find((t) => t.vector === "V(/out)")!.cursors.size, 1);
  assert.deepEqual(toWorkbook(g), wb);
  // a phase and a gain in AC
  const h = newFrame(true);
  h.circuit = circuit;
  newSimTab(h, ".ac dec 10 1 1k");
  addTrace(h, "V(/out)", SPT_VOLTAGE);
  const types = [...h.tabs[0]!.plot!.traces.values()].map((t) => t.type & 0xff).sort();
  assert.deepEqual(types, [SPT_VOLTAGE | SPT_AC_PHASE, SPT_VOLTAGE | SPT_AC_GAIN].sort());
});

test("tuners range, round and step as KiCad's", () => {
  const t = newTuner(circuit, "R1");
  assert.ok(!("error" in t));
  if ("error" in t) return;
  assert.equal(t.min.toSpiceString(), "500");
  assert.equal(t.max.toSpiceString(), "2k");
  assert.equal(t.text, "1k");
  setTunerSlider(t, 50);
  assert.equal(t.text, "1.25k");
  assert.equal(tunerText(SpiceValue.parse("1.25k")!, "E24"), "1.3k");
  assert.equal(tunerText(SpiceValue.parse("4.6k")!, "E96"), "4.64k");
  assert.equal(tunerText(SpiceValue.parse("33.3k")!, "E24"), "33k");
  assert.ok("error" in newTuner(circuit, "R9"));
  const steps = multiRunSteps([{ ...t, steps: 3, multi: true }]);
  assert.deepEqual(steps.map((s) => s.get("R1")), [500, 1250, 2000]);
});

test("the operating point goes on the longest wire of a net and on a pin", async () => {
  const { opLabels, operatingPoint, opTextSize } = await import("./opOverlay");
  const { spiceNetName } = await import("./simNames");
  assert.equal(spiceNetName("Net-(C1-Pad1)"), "Net-_C1-Pad1_");
  assert.equal(spiceNetName("/V_{OUT}"), "/VOUT");
  assert.equal(spiceNetName("//x"), "/root/x");
  const points = new Map<string, number>([["/out", 0.98946], ["r1", 1e-4]]);
  assert.equal(operatingPoint(points, "/OUT", 3, "~V"), "989mV");
  assert.equal(operatingPoint(points, "GND", 3, "~V"), "");
  assert.equal(operatingPoint(points, "/x", 3, "~V"), "?");
  assert.equal(operatingPoint(new Map(), "/x", 3, "~V"), "--");
  assert.equal(opTextSize(1), Math.round((1270 + 14) / 3));
  const c: Circuit = { ...circuit, pin_nets: { "R1.2": "/out" }, items: [{ ...circuit.items[0]!, ref: "R1" }] };
  const labels = opLabels({
    wires: [
      { net: "OUT", pins: ["R1.2"], pts: [[0, 0], [10_000, 0]] },
      { net: "OUT", pins: [], pts: [[10_000, 0], [10_000, 2_000]] },
    ],
    pins: [
      { reference: "R1", number: "1", tip: [0, -10_000], root: [0, -7_000], net: "IN" },
      { reference: "R1", number: "2", tip: [0, 0], root: [0, -3_000], net: "OUT" },
    ],
    circuit: c,
    points,
    voltages: true,
    currents: true,
    scale: 1,
  });
  const v = labels.find((l) => l.kind === "voltage")!;
  assert.deepEqual([v.text, v.x, v.h, v.v], ["989mV", 5_000, "center", "bottom"], "on the middle of the longest wire, above a horizontal one");
  const i = labels.find((l) => l.kind === "current")!;
  assert.equal(i.text, "100uA");
  assert.equal(i.vertical, false, "a vertical pin's current reads across");
});

test("the plot goes to the schematic as KiCad's image clipboard text", async () => {
  const { base64, plotImageSexpr } = await import("./simExport");
  assert.equal(base64(new Uint8Array([77, 97, 110])), "TWFu");
  assert.equal(base64(new Uint8Array([77, 97])), "TWE=");
  assert.equal(base64(new Uint8Array([77])), "TQ==");
  const s = plotImageSexpr(new Uint8Array(100).fill(255));
  const lines = s.split("\n");
  assert.equal(lines[0], "(image (at 0 0)");
  assert.equal(lines[1], "  (data");
  assert.equal(lines[2]!.length, 4 + 2 + 76, "76 characters a line");
  assert.ok(s.endsWith("  )\n)\n"));
});
