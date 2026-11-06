// The simulator's leftovers of UI-LEFT M3 against KiCad 8303b2ad's C++: the S-parameter signals (`rebuildSignalsList`), a potentiometer's tuner
// (`TUNER_SLIDER`), the multi-run cursor's run labels, snap and double click (`CURSOR::Plot`, `doSetCoordX`, `OnDoubleClick`, `updateTrace`'s
// labels), the User-defined Signals editor's completion (`onScintillaCharAdded`), the probe cursors (`cursors.cpp`), the operating-point
// overlay's formats (`RefreshOperatingPointDisplay`) and the tuner's Save (`UpdateTunerValue`: `SetParamValue`, `WriteFields`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { signalsList, type Circuit, type CircuitItem } from "./simSignals";
import { OPTION_DEFAULT_FLAGS } from "./simCommand";
import { multiRunLabels, multiRunSteps, newTuner, tunerText, type Tuner } from "./simFrame";
import { cursorRunLabel, nextCursorRun, snapCursorRun, type Trace } from "./simPlotTab";
import { USER_SIGNAL_FUNCTIONS, USER_SIGNALS_HELP, simPlotVectors, userSignalCharAdded } from "./userSignals";
import { doAutocomplete } from "../ruleCompletion";
import { probeCursorCss, xpmToSvg } from "./probeCursors";
import { DEFAULT_OP_FORMATS, opFormatsOf } from "./opOverlay";
import { tunerWrittenFields } from "./simModel";
import { decodeTables, type SimModelsJson } from "./simModelBuiltin";
import { SpiceValue } from "./spiceValue";

const item = (p: Partial<CircuitItem> & { ref: string; name: string; device: string }): CircuitItem => ({ nodes: [], pin_count: 2, pin_index: {}, currents: [`I(${p.name})`], tuner: null, ...p });
const circuit = (items: CircuitItem[]): Circuit => ({ nets: ["/in", "/out"], items, pin_nets: {}, directives: [], sch_sim_command: "", messages: [], ngspice: false });

test("an SP analysis lists S_<a>_<b> of every pair of the voltage sources' ports, amplitude and phase", () => {
  const c = circuit([
    item({ ref: "V1", name: "V1", device: "V", portnum: "1" }),
    item({ ref: "V2", name: "V2", device: "V", portnum: "2" }),
    item({ ref: "V3", name: "V3", device: "V" }),
    item({ ref: "R1", name: "R1", device: "R", portnum: "9" }),
  ]);
  const { signals } = signalsList("SP", OPTION_DEFAULT_FLAGS, c);
  assert.deepEqual(
    signals.filter((s) => s.startsWith("S_")),
    ["S_1_1 (amplitude)", "S_1_1 (phase)", "S_1_2 (amplitude)", "S_1_2 (phase)", "S_2_1 (amplitude)", "S_2_1 (phase)", "S_2_2 (amplitude)", "S_2_2 (phase)"]
  );
  // an AC analysis has none
  assert.equal(signalsList("AC", OPTION_DEFAULT_FLAGS, c).signals.filter((s) => s.startsWith("S_")).length, 0);
});

test("a potentiometer is tuned from 0 to 1, from its position or 0.5", () => {
  const pot = (value: string) => circuit([item({ ref: "RV1", name: "ARV1", device: "A", pin_count: 3, tuner: { param: "pos", value }, sim_device: "R", sim_type: "POT" })]);
  const t = newTuner(pot("0.3"), "RV1") as Tuner;
  assert.equal(t.param, "pos");
  assert.deepEqual([t.min.toDouble(), t.max.toDouble(), t.value.toDouble()], [0, 1, 0.3]);
  const u = newTuner(pot(""), "RV1") as Tuner;
  assert.equal(u.value.toDouble(), 0.5);
  assert.equal(u.text, tunerText(SpiceValue.of(0.5), ""));
  // an ideal R keeps half to twice its value
  const r = newTuner(circuit([item({ ref: "R1", name: "R1", device: "R", tuner: { param: "r", value: "1k" } })]), "R1") as Tuner;
  assert.deepEqual([r.min.toDouble(), r.max.toDouble()], [500, 2000]);
});

test("a multi-run trace's cursor names its run, snaps to the nearest run and steps through them on a double click", () => {
  const tr: Trace = { vector: "V(a)", type: 0, name: "V(a)", color: [0, 0, 0], cursors: new Map(), x: [0, 1, 2, 0, 1, 2, 0, 1, 2], y: [0, 10, 20, 100, 110, 120, 200, 210, 220], sweepCount: 3, sweepSize: 3, multiRun: true };
  const steps = multiRunSteps([{ ref: "R1", item: "R1", param: "r", min: SpiceValue.of(1000), max: SpiceValue.of(3000), value: SpiceValue.of(1000), text: "", series: "", multi: true, steps: 3 }]);
  tr.multiRunLabels = multiRunLabels(steps, 3);
  assert.deepEqual(tr.multiRunLabels, ["R1=1k", "R1=2k", "R1=3k"]);
  assert.equal(cursorRunLabel(tr, 1), "R1=2k");
  // past the labels: "Run %d"
  assert.equal(cursorRunLabel({ ...tr, multiRunLabels: ["a"] }, 2), "Run 3");
  // not a multi-run trace: no label
  assert.equal(cursorRunLabel({ ...tr, multiRun: false }, 0), null);
  // `doSetCoordX`'s snap: at x = 1.5 the runs read 15, 115, 215
  assert.equal(snapCursorRun(tr, 1.5, 120, 0), 1);
  assert.equal(snapCursorRun(tr, 1.5, -50, 2), 0);
  assert.equal(snapCursorRun(tr, 1.5, Number.NaN, 2), 2, "no target: the run it was on");
  const c = { x: 1, sweep: 2 };
  assert.equal(nextCursorRun(tr, c), true);
  assert.equal(c.sweep, 0, "round to the first");
  assert.equal(nextCursorRun({ ...tr, sweepCount: 1 }, c), false);
  // two tuners: each step's values joined by ", "
  const two = multiRunLabels([new Map([["R1", 1000], ["C1", 1e-8]])], 1);
  assert.deepEqual(two, ["R1=1k, C1=10n"]);
});

test("User-defined Signals offer the plot's vectors and ngspice's functions for the word before the caret", () => {
  const vectors = simPlotVectors({ plot_name: "tran", vectors: [{ name: "time", kind: "time", re: [] }, { name: "v(/out)", kind: "voltage", re: [] }, { name: "i(v1)", kind: "current", re: [] }, { name: "i(r1:1)", kind: "current", re: [] }] } as never);
  assert.deepEqual(vectors, ["time", "/out", "v1#branch", "i(r1:1)"]);
  // the scan: over letters, digits and '/', stopping after another character
  assert.equal(userSignalCharAdded("2*/ou", 5, vectors).partial, "/ou");
  assert.equal(userSignalCharAdded("2*V(/o", 6, vectors).partial, "V(/o");
  assert.equal(userSignalCharAdded("max(V", 5, vectors).partial, "x(V", "one '(' then the letter before it");
  assert.equal(userSignalCharAdded("sq", 2, vectors).partial, "sq", "the scan reaches the first character");
  assert.equal(userSignalCharAdded("", 0, vectors).tokens.length, 0, "at 0 nothing is offered");
  const r = userSignalCharAdded("sq", 2, vectors);
  assert.equal(r.tokens.length, vectors.length + USER_SIGNAL_FUNCTIONS.length);
  const shown = doAutocomplete(r.partial, r.tokens);
  assert.ok(shown.show && shown.list && shown.list.items[shown.list.selected] === "sqrt(x)");
  const out = doAutocomplete(userSignalCharAdded("/o", 2, vectors).partial, vectors);
  assert.ok(out.show && out.list?.items.includes("/out"));
  assert.ok(USER_SIGNALS_HELP.startsWith("SPICE functions:\n\n    sqrt(x)\n") && USER_SIGNALS_HELP.includes("    limit(nom, avar)\n"));
});

test("the probe cursors are KiCad's pictures at their hot spots", () => {
  const svg = xpmToSvg(["  .+", "++  "]);
  assert.equal(svg, '<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2" shape-rendering="crispEdges"><rect x="2" y="0" width="1" height="1" fill="#FFFFFF"/><rect x="3" y="0" width="1" height="1" fill="#000000"/><rect x="0" y="1" width="2" height="1" fill="#000000"/></svg>');
  assert.ok(probeCursorCss("voltage_probe").endsWith('") 1 31, crosshair'));
  assert.ok(probeCursorCss("current_probe").endsWith('") 4 27, crosshair'));
  assert.ok(decodeURIComponent(probeCursorCss("voltage_probe")).includes('width="32" height="32"'));
});

test("the operating-point overlay takes the project's formats, a voltage range in A made V", () => {
  assert.deepEqual(opFormatsOf(undefined), DEFAULT_OP_FORMATS);
  assert.deepEqual(opFormatsOf({ opo_v_precision: 4, opo_v_range: "mV", opo_i_precision: 2, opo_i_range: "uA" }), { vPrecision: 4, vRange: "mV", iPrecision: 2, iRange: "uA" });
  assert.equal(opFormatsOf({ opo_v_range: "mA" }).vRange, "mV", "Patch for bug early in V7.99 dev");
});

const tables = decodeTables(JSON.parse(readFileSync("src/kicad/sim_models.json", "utf8")) as SimModelsJson);
const two = [{ number: "1" }, { number: "2" }];

test("Tune's Save writes the fields as WriteFields does (r7 R7-SM-1)", () => {
  // an inferred R: the Value takes the tuned value, Sim.Device and Sim.Pins are added, no Sim.Params
  const r = tunerWrittenFields(tables, "R1", { Value: "1k", Footprint: "R_0603" }, two, "2.2k");
  assert.ok("fields" in r);
  assert.deepEqual(r.fields, { Value: "2.2k", Footprint: "R_0603", "Sim.Device": "R", "Sim.Pins": "1=+ 2=-" });
  // a DC source (`Sim.Device` V, `Sim.Type` DC) is inferred too (`InferSimModel`'s second branch), so its value goes to the Value and the
  // `Sim.Params` the tuned value replaces is erased (`GenerateParams` skips the stored primary value; `SetFieldValue( .., "" )` removes the field)
  const v = tunerWrittenFields(tables, "V1", { Value: "VDC", "Sim.Device": "V", "Sim.Type": "DC", "Sim.Params": "dc=12", "Sim.Pins": "1=+ 2=-" }, two, "9");
  assert.deepEqual(v, { fields: { Value: "9", "Sim.Device": "V", "Sim.Type": "DC", "Sim.Pins": "1=+ 2=-" } });
  // an R whose fields give its device and value: the same (the first branch keeps the Sim.Params it has and stores the value in the Value)
  const rr = tunerWrittenFields(tables, "R2", { Value: "R", "Sim.Device": "R", "Sim.Params": "r=1k", "Sim.Pins": "1=+ 2=-" }, two, "1.5k");
  assert.deepEqual(rr, { fields: { Value: "1.5k", "Sim.Device": "R", "Sim.Pins": "1=+ 2=-" } });
  // a potentiometer: its wiper
  const p = tunerWrittenFields(tables, "RV1", { Value: "10k", "Sim.Device": "R", "Sim.Type": "POT", "Sim.Pins": "1=r0 2=wiper 3=r1", "Sim.Params": "r=10k pos=0.5" }, [...two, { number: "3" }], "300m");
  assert.ok("fields" in p);
  assert.equal(p.fields["Sim.Params"], "r=10k pos=300m");
  // a SIN source is not tunable
  const s = tunerWrittenFields(tables, "V2", { Value: "SIN", "Sim.Device": "V", "Sim.Type": "SIN", "Sim.Params": "ampl=1 f=1k", "Sim.Pins": "1=+ 2=-" }, two, "1");
  assert.deepEqual(s, { error: "V2 is not tunable" });
});
