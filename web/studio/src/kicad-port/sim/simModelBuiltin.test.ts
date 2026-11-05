// The built-in simulation models (simModelBuiltin.ts, simModelDialog.ts) against KiCad 8303b2ad's C++: the tables tools/extract-sim-models.js
// copied (spot rows read off the C++ by hand), the serializer both ways, the SPICE each model generates (the Code tab's preview and the netlist's
// lines), the inference, and the dialog's choices and OK. tools/lib/simModels.test.js holds the whole tables to the C++ (`--check`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  createModelFromFields,
  createModelOfType,
  decodeTables,
  formatPwlValues,
  generateParams,
  generatePins,
  inferSimModel,
  parseParams,
  readTypeFromFields,
  referencePrefix,
  rewriteBraceExpression,
  spiceItem,
  type Fields,
  type SimModelsJson,
} from "./simModelBuiltin";
import { builtinOk, chooseDevice, chooseType, codeTab, currentBuiltin, deviceChoices, openBuiltin, paramGrid, saveInValueLabel, setGridValue, typeChoices } from "./simModelDialog";
import { convertNotation, parseSimValue, toInt, toSpice } from "./simValue";
import { gridModelColumn } from "./simModel";

const tables = decodeTables(JSON.parse(readFileSync("src/kicad/sim_models.json", "utf8")) as SimModelsJson);
const two = [{ number: "1" }, { number: "2" }];
const names = (type: string) => createModelOfType(tables, type, two).params.map((p) => p.info.name);

test("the device and type registry is KiCad's, in its iterator order (DeviceInfo, TypeInfo, SpiceInfo)", () => {
  assert.equal(tables.commit, "8303b2ada05226fa603e5ac0420d240fd65ce52b");
  assert.deepEqual(
    tables.devices.map((d) => d.name),
    ["NONE", "R", "C", "L", "K", "TLINE", "SW", "D", "NPN", "PNP", "NJFET", "PJFET", "NMES", "PMES", "NMOS", "PMOS", "V", "I", "E", "F", "G", "H", "SUBCKT", "XSPICE", "KIBIS", "SPICE"]
  );
  assert.deepEqual(tables.device.get("E"), { name: "E", fieldValue: "E", description: "Voltage Source", showInMenu: false });
  assert.deepEqual(tables.device.get("KIBIS"), { name: "KIBIS", fieldValue: "IBIS", description: "IBIS Model", showInMenu: false });
  assert.equal(tables.types.length, 107);
  const vsin = tables.type.get("V_SIN")!;
  assert.deepEqual([vsin.deviceType, vsin.fieldValue, vsin.description, vsin.class], ["V", "SIN", "Sine", "SIM_MODEL_SOURCE"]);
  assert.deepEqual(vsin.spice, { itemType: "V", modelType: "", functionName: "SIN", level: "", isDefaultLevel: false, hasExpression: false, version: "" });
  // `{ "M", "NMOS", "", "73", false, false, "2.2.0" }`
  assert.deepEqual(tables.type.get("NMOS_HISIMHV2")!.spice, { itemType: "M", modelType: "NMOS", functionName: "", level: "73", isDefaultLevel: false, hasExpression: false, version: "2.2.0" });
  assert.equal(tables.type.get("PJFET_SHICHMANHODGES")!.spice.isDefaultLevel, true);
  assert.equal(tables.type.get("D")!.ngspiceModel, "DIODE");
  assert.equal(tables.type.get("NPN_VBIC")!.ngspiceModel, "VBIC");
});

test("each class's parameters and defaults are its constructor's (ideal, sources, switches, lines, behavioural)", () => {
  assert.deepEqual(names("R"), ["r"]);
  assert.deepEqual(names("V"), ["dc", "ac", "ph", "portnum", "z0"]);
  assert.deepEqual(names("I"), ["dc", "ac", "ph"], "S-parameter ports only on a voltage source (`appendSpParamInfos`' strcmp)");
  assert.deepEqual(names("V_SIN"), ["dc", "ampl", "f", "td", "theta", "phase", "ac", "ph", "portnum", "z0"]);
  assert.deepEqual(names("V_PULSE"), ["y1", "y2", "td", "tr", "tf", "tw", "per", "np", "ac", "ph", "portnum", "z0"]);
  assert.deepEqual(names("V_EXP"), ["y1", "y2", "td1", "tau1", "td2", "tau2", "ac", "ph", "portnum", "z0"]);
  assert.deepEqual(names("V_PWL"), ["pwl", "ac", "ph", "portnum", "z0"]);
  assert.deepEqual(names("V_CCL"), ["gain", "control"]);
  assert.deepEqual(names("SW_I"), ["thr", "his", "ron", "roff", "ic"]);
  assert.deepEqual(names("TLINE_RLGC"), ["len", "r", "l", "g", "c"]);
  assert.deepEqual(names("RAWSPICE"), ["type", "model", "lib"]);
  assert.deepEqual(names("KIBIS_DRIVER_PRBS"), ["vcc", "rpin", "lpin", "cpin", "f0", "td", "n"]);
  const sin = createModelOfType(tables, "V_SIN", two).params.map((p) => [p.info.name, p.info.defaultValue, p.info.unit, p.info.category, p.info.description]);
  assert.deepEqual(sin.slice(0, 3), [
    ["dc", "", "V", "PRINCIPAL", "DC offset"],
    ["ampl", "", "V", "PRINCIPAL", "Amplitude"],
    ["f", "1/tstop", "Hz", "PRINCIPAL", "Frequency"],
  ]);
  assert.deepEqual(sin.slice(-4), [
    ["ac", "0", "V", "AC", "AC magnitude"],
    ["ph", "0", "°", "AC", "AC phase"],
    ["portnum", "", "", "S_PARAM", "Port number"],
    ["z0", "", "Ohm", "S_PARAM", "Internal impedance"],
  ]);
  // the switch's `ic`: an enum (starts at its default), an instance parameter
  const sw = createModelOfType(tables, "SW_V", two);
  assert.deepEqual([sw.params[4]!.value, sw.params[4]!.info.isSpiceInstanceParam, sw.params[4]!.info.enumValues], ["none", true, ["none", "off", "on"]]);
  assert.equal(sw.params[0]!.info.spiceModelName, "vt");
  // a field the block does not set keeps the block before's: makeCcParamInfos' "control" has the id 2, its gain the unit
  const ccl = createModelOfType(tables, "V_CCL", two).params;
  assert.deepEqual([ccl[0]!.info.unit, ccl[1]!.info.id, ccl[1]!.info.unit], ["ohm", 2, ""]);
});

test("an ngspice model takes the instance parameters of the principal, geometry and flags categories, then the model's (SIM_MODEL_NGSPICE)", () => {
  const d = createModelOfType(tables, "D", two);
  const info = tables.ngspice.DIODE!;
  assert.deepEqual([info.name, info.pinNames, info.description], ["Diode", ["A", "K"], "Junction Diode model"]);
  const inst = info.instanceParams.filter((p) => ["PRINCIPAL", "GEOMETRY", "FLAGS"].includes(p.category));
  assert.equal(d.getParamCount(), inst.length + info.modelParams.length);
  assert.deepEqual(d.params.slice(0, 3).map((p) => p.info.name), ["temp", "m", "area"]);
  // `emplace_back( "is", 101, DIR_INOUT, TYPE_FLOAT, "A", DC, "1e-14", "", "Saturation current" )`
  const is = d.findParam("is")!.info;
  assert.deepEqual([is.id, is.unit, is.category, is.defaultValue, is.description, is.isSpiceInstanceParam], [101, "A", "DC", "1e-14", "Saturation current", false]);
  assert.equal(d.findParam("m_")!.info.description, "Grading coefficient");
  assert.equal(d.findParam("M")!.info.name, "m", "the instance multiplier first; `m_` is found by its escaped name");
  // HFET1 lost its defaults: a five-argument row puts its text in the default (the constructor's next argument)
  const vt0 = tables.ngspice.HFET1!.modelParams[0]!;
  assert.deepEqual([vt0.name, vt0.defaultValue, vt0.description], ["vt0", "Pinch-off voltage", ""]);
  assert.deepEqual(createModelOfType(tables, "NPN_GUMMELPOON", two).getPinNames(), ["C", "B", "E", "<S>"]);
});

test("Sim.* fields to a model and back (SIM_MODEL_SERIALIZER)", () => {
  const fields: Fields = { Reference: "R1", Value: "x", "Sim.Device": "R", "Sim.Params": 'r="4.7k"', "Sim.Pins": "1=+ 2=-" };
  assert.equal(readTypeFromFields(tables, fields), "R");
  const r = createModelFromFields(tables, fields, two);
  assert.equal(r.getParam(0).value, "4.7k");
  const out: Fields = { ...fields };
  r.writeFields(out);
  assert.deepEqual(out, { Reference: "R1", Value: "x", "Sim.Device": "R", "Sim.Params": "r=4.7k", "Sim.Pins": "1=+ 2=-" });
  // no Sim.Params value: the Value field is the primary parameter and stays where it is
  const v = createModelFromFields(tables, { Reference: "R2", Value: "10k", "Sim.Device": "R", "Sim.Pins": "1=- 2=+" }, two);
  assert.deepEqual([v.getParam(0).value, v.isStoredInValue(), generateParams(v), generatePins(v)], ["10k", true, "", "1=- 2=+"]);
  // a source fills itself in (autofill); quoted values and flags
  const s = createModelFromFields(tables, { Reference: "V1", "Sim.Device": "V", "Sim.Type": "SIN", "Sim.Params": 'ampl=1 f="1k"', "Sim.Pins": "1=+ 2=-" }, two);
  assert.deepEqual(s.params.slice(0, 3).map((p) => p.value), ["0", "1", "1k"], "dc filled with \"0\" (`f`'s default is not a number)");
  assert.equal(generateParams(s), "dc=0 ampl=1 f=1k");
  const vd = createModelFromFields(tables, { Reference: "M1", "Sim.Device": "NMOS", "Sim.Type": "VDMOS", "Sim.Params": "thermal rd=2" }, [{ number: "1" }, { number: "2" }, { number: "3" }]);
  assert.equal(generateParams(vd), "thermal rd=2");
  // an empty value is written quoted, an enum at its default not at all
  const sw = createModelOfType(tables, "SW_V", two);
  sw.setParamValue("ic", "on");
  assert.equal(generateParams(sw), "ic=on");
  sw.setParamValue("ic", "none");
  assert.equal(generateParams(sw), "");
  // a type's model made with no fields reads the empty Value as its primary value (`ParseParams( "" )` is false, then `ParseValue( "" )`)
  const r0 = createModelOfType(tables, "R", two);
  assert.deepEqual([r0.isStoredInValue(), generateParams(r0)], [true, ""]);
  r0.setIsStoredInValue(false);
  assert.equal(generateParams(r0), 'r=""', "the primary value is written even empty when not in the Value");
});

test("the serializer's grammar errors are PEGTL's (source:line:column: message)", () => {
  const r = createModelOfType(tables, "R", two);
  const err = (s: string) => {
    try {
      parseParams(r, s);
      return "";
    } catch (e) {
      return (e as Error).message;
    }
  };
  assert.equal(err("r"), "Sim.Params field:1:2: expected '='");
  assert.equal(err("r="), "Sim.Params field:1:3: expected quoted or unquoted string");
  assert.equal(err("r=1 2"), "Sim.Params field:1:5: expected end of string", "the blank is taken by `opt< sep >`: the end is wanted at the 2");
  assert.equal(err("x=1"), "Unknown simulation model parameter 'x'");
  assert.equal(err("  r = \"1 k\"  "), "");
  assert.equal(r.getParam(0).value, "1 k");
});

test("SIM_VALUE: SI to SPICE (ToSpice), the grammar, ToInt", () => {
  assert.equal(toSpice("1k"), "1k");
  assert.equal(toSpice("1M"), "1Meg");
  assert.equal(toSpice("1e-14"), "10f");
  assert.equal(toSpice("0.001"), "0.001", "a plain number stays");
  assert.equal(toSpice("2.5u"), "2.5u");
  assert.equal(toSpice("1/tstop"), "1/tstop", "a text that is no number stays");
  assert.equal(toSpice("1P"), "1e+15", "SPICE has no peta");
  assert.equal(convertNotation("3,3m", "SI", "SI"), "3.3m");
  assert.equal(parseSimValue("1e").isOk, false);
  assert.equal(parseSimValue("10Meg", "SPICE").unitPrefixExponent, 6);
  assert.equal(toInt("2k"), 2000);
  assert.equal(toInt("1.5"), -1);
});

test("the Code tab (SPICE_GENERATOR::Preview with the reference as the model name): no item pins, so each model pin is NC", () => {
  const fields: Fields = { Reference: "R1" };
  const r = createModelOfType(tables, "R", two);
  r.setParamValue("r", "1k");
  assert.equal(codeTab(r, fields, true), "R NC--0 NC--1 1k");
  const c = createModelOfType(tables, "C", two);
  assert.equal(codeTab(c, { Reference: "C1" }, true), "", "no value, no line");
  const v = createModelOfType(tables, "V_SIN", two);
  setGridValue(v, 1, "1");
  setGridValue(v, 2, "1k");
  assert.equal(codeTab(v, { Reference: "V1" }, true), "V NC--0 NC--1 DC 0 SIN( 0 1 1k )");
  const d = createModelOfType(tables, "D", two);
  assert.equal(codeTab(d, { Reference: "D1" }, true), ".model D1 D\n\n\nD NC--0 NC--1 D1");
  d.setParamValue("is", "1e-14");
  d.setParamValue("n", "1.8");
  d.setParamValue("m", "2");
  assert.equal(codeTab(d, { Reference: "D1" }, true), ".model D1 D\n+         is=10f\n+         n=1.8\n\n\nD NC--0 NC--1 D1 m=2");
  const q = createModelOfType(tables, "PNP_VBIC", [{ number: "1" }, { number: "2" }, { number: "3" }]);
  assert.equal(codeTab(q, { Reference: "Q1" }, true), ".model Q1 PNP level=4\n\n\nQ NC--0 NC--1 NC--2 Q1");
  const p = createModelOfType(tables, "V_PULSE", two);
  setGridValue(p, 1, "5");
  assert.equal(codeTab(p, { Reference: "V2" }, true), "V NC--0 NC--1 PULSE( 0 5 0 1 )", "a single pulse when neither PER nor NP is given (ngspice 46)");
  const raw = createModelOfType(tables, "RAWSPICE", two);
  raw.setParamValue("model", "1k");
  assert.equal(codeTab(raw, { Reference: "X1" }, true), " 1k");
});

test("the netlist's lines for a SPICE item (ModelName, ModelLine, ItemLine) -- what kicad-cli writes", () => {
  const item = (ref: string, nets: string[], model: { generator: { modelName: (i: ReturnType<typeof spiceItem>) => string } }) => {
    const it = spiceItem({ refName: ref, pinNumbers: nets.map((_, k) => String(k + 1)), pinNetNames: nets });
    it.modelName = model.generator.modelName(it);
    return it;
  };
  const r = createModelFromFields(tables, { Reference: "R1", "Sim.Device": "R", "Sim.Params": "r=1k", "Sim.Pins": "1=+ 2=-" }, two);
  const ri = item("R1", ["/in", "/out"], r);
  assert.equal(ri.modelName, "__R1");
  assert.deepEqual([r.generator.modelLine(ri), r.generator.itemLine(ri)], ["", "R1 /in /out 1k\n"]);
  const d = createModelFromFields(tables, { Reference: "D1", "Sim.Device": "D", "Sim.Params": "is=1e-14 n=1.8", "Sim.Pins": "1=K 2=A" }, two);
  const di = item("D1", ["/out", "GND"], d);
  assert.equal(d.generator.modelLine(di), ".model __D1 D\n+           is=10f\n+           n=1.8\n");
  assert.equal(d.generator.itemLine(di), "D1 GND /out __D1\n", "the pins in the model's order: A is symbol pin 2");
  const sw = createModelFromFields(tables, { Reference: "W1", "Sim.Device": "SW", "Sim.Type": "I", "Sim.Pins": "1=ctrl+ 2=ctrl- 3=no+ 4=no-" }, [{ number: "1" }, { number: "2" }, { number: "3" }, { number: "4" }]);
  const wi = item("W1", ["a", "b", "c", "d"], sw);
  assert.equal(sw.generator.itemLine(wi), "a b 0\nW1 c d V__W1 __W1 1\n", "a current switch measures through V__<ref>; its `ic` default is \"1\"");
});

test("PWL points and brace expressions in SPICE notation (formatPwlValues, rewriteBraceExpression)", () => {
  assert.equal(formatPwlValues("0 0 1u 0 2u 1M"), "0 0 1u 0 2u 1Meg ");
  assert.equal(rewriteBraceExpression("{t_start + 1M}"), "{t_start + 1Meg}");
  assert.equal(rewriteBraceExpression("{x1M + 2.5k}"), "{x1M + 2.5k}");
  assert.equal(formatPwlValues("{t0 + 1u} 5"), "{t0 + 1u} 5 ");
});

test("InferSimModel: a passive or a DC source from its prefix and Value", () => {
  const inf = (prefix: string, value: string, extra: Fields = {}) => inferSimModel(prefix, { Value: value, ...extra }, two);
  assert.deepEqual(inf("R", "4k7"), { deviceType: "R", modelType: "", modelParams: 'r="4.7k"', pinMap: "1=+ 2=-" });
  assert.deepEqual(inf("C", "100n")!.modelParams, 'c="100n"');
  assert.deepEqual(inf("R", "1,5"), { deviceType: "R", modelType: "", modelParams: 'r="1.5"', pinMap: "1=+ 2=-" }, "a ',' before other than 3 digits is a decimal point");
  assert.deepEqual(inf("R", "2MEG")!.modelParams, 'r="2M"');
  assert.deepEqual(inf("R", "10 µ")!.modelParams, 'r="10u"');
  assert.deepEqual(inf("L", "V(in)*2"), { deviceType: "L", modelType: "=", modelParams: 'l="V(in)*2"', pinMap: "1=+ 2=-" }, "behavioural");
  assert.deepEqual(inf("V", "DC 5"), { deviceType: "V", modelType: "DC", modelParams: 'dc="5" ', pinMap: "1=+ 2=-" });
  assert.deepEqual(inf("I", "AC 1m")!.modelParams, 'ac="1m" ');
  assert.equal(inf("U", "10k"), null);
  assert.equal(inferSimModel("R", { Value: "1k" }, [{ number: "1" }]), null, "two pins only");
  assert.equal(referencePrefix("R12"), "R");
  assert.equal(referencePrefix("SW?"), "SW");
});

test("the dialog on an inferred resistor: built-in, Ideal, the primary value saved in Value, OK writes the model as KiCad does", () => {
  const pins = [{ number: "1", name: "" }, { number: "2", name: "" }];
  const o = openBuiltin(tables, "R", { Reference: "R1", Value: "10k" }, pins);
  assert.deepEqual([o.radio, o.inferred, o.saveInValue, o.builtin.curModelType, o.fields.Value], ["builtin", true, true, "R", "${SIM.PARAMS}"]);
  const m = currentBuiltin(o.builtin);
  assert.equal(m.getParam(0).value, "10k");
  const dev = deviceChoices(tables, m);
  assert.deepEqual(dev.items.slice(0, 4), ["", "Resistor", "Capacitor", "Inductor"]);
  assert.equal(dev.items[dev.selected], "Resistor");
  assert.ok(!dev.items.includes("IBIS Model") && !dev.items.includes("Subcircuit"), "not in the menu");
  assert.deepEqual(typeChoices(tables, m), { items: ["Ideal", "Potentiometer", "Behavioral"], selected: 0, shown: true });
  assert.deepEqual(saveInValueLabel(m), { label: "Save parameter 'Resistance (r)' in Value field", enabled: true });
  const column = gridModelColumn(m.getPins(), pins);
  assert.deepEqual(column, ['1 ("+")', '2 ("-")']);
  const out = builtinOk(m, o.fields, { saveInValue: true, column, pins });
  assert.deepEqual(out, { Reference: "R1", Value: "10k", "Sim.Device": "R", "Sim.Pins": "1=+ 2=-" });
  // the same with "Save in Value" off: the Value stays "${SIM.PARAMS}" and the parameter is in Sim.Params
  const o2 = openBuiltin(tables, "R", { Reference: "R1", Value: "10k" }, pins);
  assert.deepEqual(builtinOk(currentBuiltin(o2.builtin), o2.fields, { saveInValue: false, column, pins }), { Reference: "R1", Value: "${SIM.PARAMS}", "Sim.Device": "R", "Sim.Params": "r=10k", "Sim.Pins": "1=+ 2=-" });
});

test("the dialog: a voltage source's types, a device's remembered type, the parameter grid's categories", () => {
  const pins = [{ number: "1" }, { number: "2" }];
  const o = openBuiltin(tables, "V", { Reference: "V1", Value: "VSIN" }, pins);
  assert.deepEqual([o.radio, o.inferred, o.builtin.curModelType], ["builtin", true, "V"], "a V prefix with a Value is a DC source (its value text)");
  chooseType(tables, o.builtin, "Sine");
  assert.equal(o.builtin.curModelType, "V_SIN");
  const vsin = currentBuiltin(o.builtin);
  const t = typeChoices(tables, vsin);
  assert.deepEqual(t.items.slice(0, 9), ["DC", "Sine", "Pulse", "Exponential", "Amplitude modulated", "Single-frequency FM", "Voltage-controlled", "Current-controlled", "Piecewise linear"]);
  assert.equal(t.items[t.selected], "Sine");
  assert.deepEqual(saveInValueLabel(vsin), { label: "Save primary parameter in Value field", enabled: false });
  const grid = paramGrid(vsin);
  assert.deepEqual(grid.principal.map((r) => r.label).slice(0, 3), ["DC offset (dc)", "Amplitude (ampl)", "Frequency (f)"]);
  assert.deepEqual(grid.groups.map((g) => [g.title, g.expanded, g.rows.map((r) => r.name)]), [
    ["AC", true, ["ac", "ph"]],
    ["S-Parameters", false, ["portnum", "z0"]],
  ]);
  chooseDevice(tables, o.builtin, "Diode");
  assert.equal(o.builtin.curModelType, "D");
  chooseDevice(tables, o.builtin, "Voltage Source");
  assert.equal(o.builtin.curModelType, "V_SIN", "the device keeps the type chosen last");
  const d = paramGrid(o.builtin.models.get("D")!);
  assert.deepEqual(d.groups.map((g) => g.title), ["Geometry", "DC", "Capacitance", "Temperature", "Noise", "Limiting Values"]);
  assert.equal(d.groups[1]!.rows[0]!.label, "Saturation current (is)");
  assert.deepEqual([d.groups[1]!.rows[0]!.unit, d.groups[1]!.rows[0]!.default, d.groups[1]!.rows[0]!.type], ["A", "1e-14", "Float"]);
  const mos = paramGrid(o.builtin.models.get("NMOS_VDMOS")!);
  assert.ok(mos.groups.some((g) => g.title === "Flags" && g.rows.some((r) => r.kind === "bool")), "VDMOS's flags are checkboxes");
});
