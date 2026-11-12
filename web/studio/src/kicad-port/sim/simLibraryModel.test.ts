// The Simulation Model dialog's round v10 (UI-LEFT-2 MM1, MM2) against KiCad 8303b2ad's C++: a library model's parameters
// (`SPICE_MODEL_PARSER::ReadModel`, `SIM_MODEL_NGSPICE::SetParamFromSpiceCode`, `SPICE_MODEL_PARSER_SUBCKT::ReadModel`, `SIM_MODEL::Create(
// aBaseModel, .. )`), the grid's arithmetic (`SIM_STRING_PROPERTY`), the K device's inductors (`newParamProperty`), the fields' visibility
// (`SetFieldValue`) and Symbol Properties' merge (`OnEditSpiceModel`), the resolved texts, and 25ec2335's `RestoreInferredValue`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { decodeTables, generateParams, SimModelSpiceFallback, SimModelSubckt, spiceItem, type SimModelsJson } from "./simModelBuiltin";
import { baseModelOf, cardTokens, joinCard, libraryBaseModels, libraryModel, modelCardParams, subcktParams } from "./simLibraryModel";
import { builtinOk, codeTab, currentBuiltin, editedFieldsMerge, gridText, inductorChoices, openBuiltin, paramGrid } from "./simModelDialog";
import { gridModelColumn } from "./simModel";

const tables = decodeTables(JSON.parse(readFileSync("src/kicad/sim_models.json", "utf8")) as SimModelsJson);
const two = [{ number: "1" }, { number: "2" }];

const LIB = [
  { name: "D1N4148", kind: "model" as const, modelType: "D", pins: [], code: ".model D1N4148 D(IS=2.52n RS=0.568 N=1.752 ; a comment\n+ CJO=4p M=0.4 tt=20n mfg=OnSemi)" },
  { name: "D2", kind: "model" as const, modelType: "D", pins: [], code: ".model D2 ako: D1N4148 D n=2" },
  { name: "DBAD", kind: "model" as const, modelType: "D", pins: [], code: ".model DBAD D(is=1n zork=3)" },
  { name: "AMP", kind: "subckt" as const, pins: ["inp", "inn", "out"], code: ".subckt AMP inp inn out params: gain=10 rin={1k*2}\nE1 out 0 inp inn {gain}\n.ends" },
];

test("the card's tokens are SPICE_GRAMMAR's: '=', '(', ')' and ',' separate, '+' lines continue, ';' and '$' end a line, braces whole", () => {
  assert.equal(joinCard(".model X D(is=1\n* a comment line\n+ n=2 ; tail\n+ rs=3 $ more"), ".model X D(is=1  n=2   rs=3 ");
  assert.deepEqual(cardTokens(".model X D(is=1e-14, rs={a + b}) bv=[1 2]"), [".model", "X", "D", "is", "1e-14", "rs", "{a + b}", "bv", "[1 2]"]);
  assert.deepEqual(modelCardParams(".model M1 VDMOS NCHAN (vto=2 kp=5)").params, [["vto", "2"], ["kp", "5"]]);
  assert.deepEqual(modelCardParams(".model Q3 ako: QNPN NPN bf=100"), { params: [["bf", "100"]], ok: true, ako: "QNPN" });
  assert.equal(modelCardParams(".model X D(is=1 2)").ok, false, "a value with no name is PEGTL's parse error");
  assert.deepEqual(subcktParams(LIB[3]!.code), [["gain", "10"], ["rin", "{1k*2}"]]);
});

test("a library .model's parameters are the library's (ReadModel, SetParamFromSpiceCode), an ako: model takes its parent's first", () => {
  const bases = libraryBaseModels(tables, LIB);
  const d = bases.get("D1N4148")!;
  assert.equal(d.type, "D");
  const value = (name: string) => d.params.find((p) => p.info.name === name)?.value;
  // SPICE notation read into SI; mfg is one of the parameters KiCad lets pass; M is the model's grading coefficient, escaped "m_" (the
  // instance's "m" is not a .model card's)
  assert.deepEqual([value("is"), value("rs"), value("n"), value("cjo"), value("m_"), value("m"), value("tt")], ["2.52n", "0.568", "1.752", "4p", "0.4", "", "20n"]);
  const d2 = bases.get("D2")!;
  assert.deepEqual([d2.params.find((p) => p.info.name === "is")?.value, d2.params.find((p) => p.info.name === "n")?.value], ["2.52n", "2"]);
  // an unknown parameter makes ReadModel throw: the model is kept as its code, with no parameters (SIM_MODEL_SPICE_FALLBACK)
  const bad = bases.get("DBAD")!;
  assert.ok(bad instanceof SimModelSpiceFallback);
  assert.equal(bad.getParamCount(), 0);
  assert.equal(bad.spiceCode, LIB[2]!.code);
});

test("the dialog's model of a library model: the grid shows the library's values, Sim.Params only what the instance changes", () => {
  const bases = libraryBaseModels(tables, LIB);
  const fields = { Reference: "D1", Value: "1N4148", "Sim.Library": "diodes.lib", "Sim.Name": "D1N4148", "Sim.Device": "D", "Sim.Pins": "1=K 2=A", "Sim.Params": "n=1.9 area=2" };
  const m = libraryModel(tables, bases.get("D1N4148")!, two, fields);
  const grid = paramGrid(m);
  const rows = [...grid.principal, ...grid.groups.flatMap((g) => g.rows)];
  assert.equal(rows.find((r) => r.name === "is")?.value, "2.52n", "the base model's value through GetParam");
  assert.equal(rows.find((r) => r.name === "n")?.value, "1.9", "the instance's override");
  assert.deepEqual(m.getPins().map((p) => [p.modelPinName, p.symbolPinNumber]), [["A", "2"], ["K", "1"]]);
  // OK: Sim.Library and Sim.Name, Sim.Device D, the pins, and the overrides only (GenerateParams leaves out the base model's values)
  const out = builtinOk(m, fields, { saveInValue: false, column: gridModelColumn(m.getPins(), [{ number: "1", name: "" }, { number: "2", name: "" }]), pins: two, library: { path: "diodes.lib", name: "D1N4148" } });
  assert.deepEqual(out, { Reference: "D1", Value: "1N4148", "Sim.Library": "diodes.lib", "Sim.Name": "D1N4148", "Sim.Device": "D", "Sim.Pins": "1=K 2=A", "Sim.Params": "area=2 n=1.9" });
  assert.equal(generateParams(libraryModel(tables, bases.get("D1N4148")!, two, null)), "", "another model of the library: no overrides");
  // the Code tab: the model line made again (with the library's values) under the list's name, the item line
  const code = codeTab(m, fields, false, "D1N4148");
  assert.match(code, /^\.model D1N4148 D\n\+\s+is=2\.52n\n/);
  assert.match(code, /\n\nD NC--0 NC--1 D1N4148 area=2$/);
});

test("a library subcircuit: its ports and params: (SPICE_MODEL_PARSER_SUBCKT::ReadModel), copied to the dialog's model (SetBaseModel)", () => {
  const base = baseModelOf(tables, LIB[3]!, () => null);
  assert.ok(base instanceof SimModelSubckt);
  const m = libraryModel(tables, base, [{ number: "1" }, { number: "2" }, { number: "3" }], null);
  assert.deepEqual(m.getPins().map((p) => [p.modelPinName, p.symbolPinNumber]), [["inp", "1"], ["inn", "2"], ["out", "3"]]);
  const rows = paramGrid(m).principal;
  assert.deepEqual(rows.map((r) => [r.name, r.value, r.default, r.type]), [["gain", "", "10", "Float"], ["rin", "", "{1k*2}", "Float"]]);
  m.setParamValue("gain", "20");
  // the item line's instance parameter; the preview is the subcircuit's code, then the item line
  const preview = m.generator.preview(spiceItem({ modelName: "AMP" }));
  assert.ok(preview.startsWith(".subckt AMP inp inn out"));
  assert.match(preview, /\.ends\n\nX NC--0 NC--1 NC--2 AMP gain=20$/);
  assert.equal(generateParams(m), "gain=20");
});

test("the grid's arithmetic (SIM_STRING_PROPERTY, NUMERIC_EVALUATOR in SI mode): normalized on Enter, the result on leaving the row", () => {
  assert.equal(gridText("1k*2", "TYPE_FLOAT", "enter"), "2k");
  assert.equal(gridText("1k*2", "TYPE_FLOAT", "blur"), "2000");
  assert.equal(gridText("10u", "TYPE_FLOAT", "enter"), "10u", "works out to what it is: kept as typed");
  // "4k7" is no SIM_VALUE (`ToDouble` gives NaN), so it is not Equal to its result and comes out normalized, as in KiCad
  assert.equal(gridText("4k7", "TYPE_FLOAT", "enter"), "4.7k");
  assert.equal(gridText("1/(2*3)", "TYPE_FLOAT", "enter"), "166.667m");
  assert.equal(gridText("abc+", "TYPE_FLOAT", "enter"), "abc+", "does not work out");
  assert.equal(gridText("1k*2", "TYPE_STRING", "enter"), "1k*2", "a text row takes no arithmetic");
  assert.equal(gridText("3*4", "TYPE_INT", "blur"), "12");
});

test("the K device's l1 and l2 are a choice of the circuit's inductors, none selected (newParamProperty, SIM_ENUM_PROPERTY)", () => {
  const inductors = inductorChoices([
    { ref: "L10", sim_device: "L" },
    { ref: "R1", sim_device: "R" },
    { ref: "L2", sim_device: "L" },
  ]);
  assert.deepEqual(inductors, ["L2", "L10"]);
  const o = openBuiltin(tables, "K", { Reference: "K1", Value: "K", "Sim.Device": "K", "Sim.Params": "l1=L2 l2=L10 k=0.9" }, two);
  const m = currentBuiltin(o.builtin);
  const rows = paramGrid(m, inductors).principal;
  const l1 = rows.find((r) => r.name === "l1")!;
  assert.deepEqual([l1.kind, l1.choices, l1.selected, l1.value], ["enum", ["L2", "L10"], -1, "L2"]);
  assert.equal(paramGrid(m, []).principal.find((r) => r.name === "l1")!.kind, "string", "no inductor: text");
});

test("the fields the dialog adds are visible as SetFieldValue makes them; Symbol Properties takes them (OnEditSpiceModel)", () => {
  const pins = [{ number: "1", name: "" }, { number: "2", name: "" }];
  const o = openBuiltin(tables, "R", { Reference: "R1", Value: "4k7" }, pins);
  assert.deepEqual(o.visible, { "Sim.Device": true, "Sim.Params": true, "Sim.Pins": true }, "the inference's fields are visible");
  const visible = { ...o.visible };
  const m = currentBuiltin(o.builtin);
  const out = builtinOk(m, o.fields, { saveInValue: true, column: gridModelColumn(m.getPins(), pins), pins, visible, inferredValueRestore: o.inferredValueRestore });
  assert.deepEqual(out, { Reference: "R1", Value: "4.7k", "Sim.Device": "R", "Sim.Pins": "1=+ 2=-" });
  const merge = editedFieldsMerge({ Reference: "R1", Value: "4k7", Note: "x" }, out, visible);
  assert.deepEqual(merge, { set: [], add: [{ name: "Sim.Device", text: "R", visible: true }, { name: "Sim.Pins", text: "1=+ 2=-", visible: true }], remove: ["Note"] });
  // a library model: Sim.Library and Sim.Name hidden; WriteFields' own hidden
  const v2: Record<string, boolean> = {};
  const lib = libraryModel(tables, libraryBaseModels(tables, LIB).get("D1N4148")!, two, null);
  builtinOk(lib, { Reference: "D1", Value: "D" }, { saveInValue: false, column: ["Not Connected", "Not Connected"], pins: two, visible: v2, library: { path: "d.lib", name: "D1N4148" } });
  assert.deepEqual(v2, { "Sim.Library": false, "Sim.Name": false, "Sim.Device": false, "Sim.Pins": false });
});

test("KiCad 25ec2335's RestoreInferredValue: an inferred model not saved in the Value gets its Value back", () => {
  const pins = [{ number: "1", name: "" }, { number: "2", name: "" }];
  const o = openBuiltin(tables, "R", { Reference: "R1", Value: "4k7" }, pins);
  assert.equal(o.inferredValueRestore, "4k7");
  const m = currentBuiltin(o.builtin);
  const out = builtinOk(m, o.fields, { saveInValue: false, column: gridModelColumn(m.getPins(), pins), pins, inferredValueRestore: o.inferredValueRestore });
  assert.deepEqual(out, { Reference: "R1", Value: "4k7", "Sim.Device": "R", "Sim.Params": "r=4.7k", "Sim.Pins": "1=+ 2=-" });
});

test("the fields are read resolved (GetFieldValue( .., aResolve = true )): a ${VAR} in Sim.Params is its value", () => {
  const shown: Record<string, string> = { "r=${RVAL}": "r=2.2k" };
  const o = openBuiltin(tables, "R", { Reference: "R1", Value: "x", "Sim.Device": "R", "Sim.Params": "r=${RVAL}" }, two, (t) => shown[t] ?? t);
  assert.equal(currentBuiltin(o.builtin).getParam(0).value, "2.2k");
  assert.deepEqual(o.errors, []);
});

test("MM3: a net drawn in pieces gets an OP label on each piece's longest wire (RefreshOperatingPointDisplay over GetNetMap's subgraphs)", async () => {
  const { opLabels, netPieces } = await import("./opOverlay");
  const wires = [
    { net: "OUT", pins: ["R1.2"], pts: [[0, 0], [10_000, 0]] as [number, number][] },
    { net: "OUT", pins: [], pts: [[10_000, 0], [10_000, 2_000]] as [number, number][] },
    // the same net again, joined by a label only: its own piece
    { net: "OUT", pins: [], pts: [[50_000, 0], [54_000, 0]] as [number, number][] },
    // an end on the middle of the first piece's wire: the first piece
    { net: "OUT", pins: [], pts: [[5_000, 0], [5_000, 9_000]] as [number, number][] },
  ];
  assert.deepEqual(netPieces(wires).map((p) => p.length), [3, 1]);
  const circuit = { nets: [], items: [], pin_nets: { "R1.2": "/out" }, directives: [] } as never;
  const labels = opLabels({ wires, pins: [], circuit, points: new Map([["/out", 0.98946]]), voltages: true, currents: false, scale: 1 });
  assert.deepEqual(labels.map((l) => [l.text, l.x]), [["989mV", 5_000], ["989mV", 52_000]]);
});

test("MM3: before a run User-defined Signals offer libngspice's const plot (SimPlotVectors of the current plot)", async () => {
  const { simPlotVectors, CONST_PLOT_VECTORS } = await import("./userSignals");
  assert.deepEqual(simPlotVectors(undefined), [...CONST_PLOT_VECTORS]);
  assert.ok(simPlotVectors(undefined).includes("pi") && simPlotVectors(undefined).includes("boltz"));
});
