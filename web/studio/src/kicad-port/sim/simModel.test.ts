import { test } from "node:test";
import assert from "node:assert/strict";
import { assignRow, defaultModelPins, fieldsOnOk, fieldVerbs, filterModelNames, generatePins, gridModelColumn, initialModel, modelPinChoices, modelPinIndex, NOT_CONNECTED, parsePins, pinsOnOk, symbolPinString, type LibraryModelInfo } from "./simModel";

const amp: LibraryModelInfo = { name: "MYAMP", kind: "subckt", device: "SUBCKT", type: "", pins: ["inp", "inn", "vcc", "vee", "out"], code: "" };
const npn: LibraryModelInfo = { name: "Q2N", kind: "model", device: "NPN", type: "GUMMELPOON", pins: ["C", "B", "E", "<S>"], code: "" };
const sym = [
  { number: "1", name: "+" },
  { number: "2", name: "-" },
  { number: "3", name: "V+" },
  { number: "4", name: "V-" },
  { number: "5", name: "5" },
];

test("a subcircuit's n-th port is on symbol pin n; a device's pins on the sorted symbol pins, an optional one left out", () => {
  assert.deepEqual(defaultModelPins(amp, sym).map((p) => p.symbolPinNumber), ["1", "2", "3", "4", "5"]);
  const three = [{ number: "1", name: "C" }, { number: "2", name: "B" }, { number: "3", name: "E" }];
  assert.deepEqual(defaultModelPins(npn, three), [
    { modelPinName: "C", symbolPinNumber: "1" },
    { modelPinName: "B", symbolPinNumber: "2" },
    { modelPinName: "E", symbolPinNumber: "3" },
  ]);
  const d: LibraryModelInfo = { ...npn, pins: ["A", "K", "X"] };
  assert.deepEqual(defaultModelPins(d, [{ number: "1", name: "" }]).map((p) => p.symbolPinNumber), ["1", "", ""], "a required pin past the symbol's stays unconnected");
});

test("Sim.Pins assigns by model pin name, else by its 1-based index; an unknown pin is KiCad's error", () => {
  const p = parsePins(defaultModelPins(amp, sym), "1=inn 2=inp 3=3");
  assert.deepEqual(p.map((x) => x.symbolPinNumber), ["2", "1", "3", "4", "5"]);
  let said = "";
  try {
    parsePins(defaultModelPins(amp, sym), "1=nope");
  } catch (e) {
    said = (e as Error).message;
  }
  assert.equal(said, "Unknown simulation model pin 'nope'");
});

test("the grid's strings and a cell change (getSymbolPinString, getModelPinString, onPinAssignmentsGridCellChange)", () => {
  assert.equal(symbolPinString(sym[0]!), '1 ("+")');
  assert.equal(symbolPinString(sym[4]!), "5");
  const pins = defaultModelPins(amp, sym);
  const col = gridModelColumn(pins, sym);
  assert.deepEqual(col, ['1 ("inp")', '2 ("inn")', '3 ("vcc")', '4 ("vee")', '5 ("out")']);
  assert.equal(modelPinChoices(pins).at(-1), NOT_CONNECTED);
  assert.equal(modelPinIndex('5 ("out")'), 4);
  // row 1 (symbol pin "1") takes model pin 5: pin 1 is let go, and the row that had pin 5 shows Not Connected
  const after = assignRow(pins, sym, 0, col[0]!, col[4]!);
  assert.deepEqual(gridModelColumn(after, sym), ['5 ("out")', '2 ("inn")', '3 ("vcc")', '4 ("vee")', NOT_CONNECTED]);
});

test("OK writes Sim.Pins sorted by symbol pin, the orphans let go, and KiCad's library fields", () => {
  const pins = [
    { modelPinName: "out", symbolPinNumber: "10" },
    { modelPinName: "inp", symbolPinNumber: "2" },
    { modelPinName: "inn", symbolPinNumber: "" },
  ];
  assert.equal(generatePins(pins), "2=inp 10=out");
  const ok = pinsOnOk(defaultModelPins(amp, sym.slice(0, 3)), sym.slice(0, 3), ['1 ("inp")', '2 ("inn")', '3 ("vcc")']);
  assert.equal(generatePins(ok), "1=inp 2=inn 3=vcc", "ports on symbol pins 4 and 5 (the symbol has none) are let go");
  const wanted = fieldsOnOk({ "Sim.Params": "gain=2", "Sim.Library": "a.lib", "Sim.Name": "OLD" }, "models/amp.lib", amp, defaultModelPins(amp, sym));
  assert.deepEqual(wanted, { "Sim.Library": "models/amp.lib", "Sim.Name": "MYAMP", "Sim.Device": "SUBCKT", "Sim.Type": "", "Sim.Enable": "", "Sim.Pins": "1=inp 2=inn 3=vcc 4=vee 5=out", "Sim.Params": "" });
  assert.equal(fieldsOnOk({ "Sim.Params": "gain=2", "Sim.Library": "models/amp.lib", "Sim.Name": "MYAMP" }, "models/amp.lib", amp, [])["Sim.Params"], "gain=2", "the same model keeps its overrides");
});

test("the verbs: set what changes, remove the instance's emptied fields, empty the library's", () => {
  const current = { "Sim.Device": "R", "Sim.Params": "r=1k", "Sim.Type": "POT" };
  const v = fieldVerbs("U1", current, ["Sim.Params"], { "Sim.Device": "SUBCKT", "Sim.Params": "", "Sim.Type": "", "Sim.Name": "X", "Sim.Enable": "" });
  assert.deepEqual(v.removes, ["Sim.Params"]);
  assert.deepEqual(v.edits, [
    { id: "U1", field: "Sim.Device", value: "SUBCKT" },
    { id: "U1", field: "Sim.Type", value: "" },
    { id: "U1", field: "Sim.Name", value: "X" },
  ]);
});

test("the model the dialog opens on, and the filter", () => {
  assert.deepEqual(initialModel(["B", "A"], "B", "x.lib"), { name: "B", message: null });
  assert.deepEqual(initialModel(["B", "A"], "C", "x.lib"), { name: "A", message: "No model named 'C' in 'x.lib'." });
  assert.deepEqual(filterModelNames(["LM358", "TL072", "LM741"], "LM"), ["LM358", "LM741"]);
  assert.deepEqual(filterModelNames(["LM358", "lm741"], "lm"), ["lm741"], "Matches is case-sensitive");
});

test("Tune's Save: the Sim.Params parameter when the fields have it, else Value; one set_symbol_fields edit (r7 R7-SM-2)", async () => {
  const { tunerSaveEdit } = await import("./simModel");
  assert.deepEqual(tunerSaveEdit("R1", "r", "15k", {}), { id: "R1", field: "Value", value: "15k" });
  assert.deepEqual(tunerSaveEdit("RV1", "r", "4k7", { "Sim.Params": "r=10k pos=0.5" }), { id: "RV1", field: "Sim.Params", value: "r=4k7 pos=0.5" });
  assert.deepEqual(tunerSaveEdit("R2", "r", "1k", { "Sim.Params": "tc1=0.1" }), { id: "R2", field: "Value", value: "1k" });
});
