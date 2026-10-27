import { test } from "node:test";
import assert from "node:assert/strict";
import type { Schematic } from "../../api/types";
import { pinItems } from "../schPins";
import { expandOpVars, labelOpToken, netAt, symbolOpToken, withOpVars } from "./opVars";
import type { CircuitItem } from "./simSignals";

const points = new Map<string, number>([["/out", 0.98946], ["r1", 0.0012], ["r1:power", 0.002], ["q1:c", 0.0005]]);
const r1: CircuitItem = { ref: "R1", name: "R1", device: "R", nodes: ["/in", "/out"], pin_count: 2, pin_index: { "1": 0, "2": 1 }, currents: ["I(R1)"], tuner: null };
const q1: CircuitItem = { ref: "Q1", name: "Q1", device: "Q", nodes: ["/c", "/b", "0"], pin_count: 3, pin_index: { "1": 0, "2": 1, "3": 2 }, currents: ["I(Q1:c)", "I(Q1:b)", "I(Q1:e)"], tuner: null };

test("S1: a label's ${OP} is its net's operating point (SCH_LABEL_BASE::ResolveTextVar: 3 digits, ~V)", () => {
  assert.equal(expandOpVars("OUT ${OP}", (t) => labelOpToken(t, { points, net: "/OUT" })), "OUT 989mV");
  assert.equal(expandOpVars("${OP.2}", (t) => labelOpToken(t, { points, net: "/OUT" })), "990mV");
  assert.equal(expandOpVars("${OP}", (t) => labelOpToken(t, { points: new Map(), net: "/OUT" })), "--", "no run yet");
  assert.equal(expandOpVars("${OP}", (t) => labelOpToken(t, { points, net: null })), "?", "no connection");
  assert.equal(expandOpVars("\\${OP} ${NET_NAME}", (t) => labelOpToken(t, { points, net: "/OUT" })), "\\${OP} ${NET_NAME}", "escaped and other variables are left");
});

test("S1: a symbol's ${OP} is its current, ${OP:power} its power, ${OP:<pin>} a model pin's current (SCH_SYMBOL::ResolveTextVar)", () => {
  const pins = [{ number: "1", name: "C" }, { number: "2", name: "B" }, { number: "3", name: "E" }];
  assert.equal(expandOpVars("${OP}", (t) => symbolOpToken(t, { points, ref: "R1", item: r1, pins: [] })), "1.20mA");
  assert.equal(expandOpVars("${OP:power}", (t) => symbolOpToken(t, { points, ref: "R1", item: r1, pins: [] })), "2.00mW");
  assert.equal(expandOpVars("${OP:1}", (t) => symbolOpToken(t, { points, ref: "R1", item: r1, pins: [{ number: "1", name: "~" }] })), "1.20mA", "a two-pin model: the item's current");
  assert.equal(expandOpVars("${OP:c}", (t) => symbolOpToken(t, { points, ref: "Q1", item: q1, pins })), "500uA", "by the pin's name, case ignored");
  assert.equal(expandOpVars("${OP:9}", (t) => symbolOpToken(t, { points, ref: "Q1", item: q1, pins })), "?");
  assert.equal(expandOpVars("${VALUE}", (t) => symbolOpToken(t, { points, ref: "R1", item: r1, pins: [] })), "${VALUE}");
});

// r3 S3-1: a label on a pin with no wire takes the pin's net (`updateGenericItemConnectivity`); a sheet pin resolves `${OP}` as the hierarchical label it is.
test("withOpVars: a label on a pin takes the pin's net, a label mid-wire the wire's, and a sheet pin resolves too", () => {
  const libPin = (number: string, y: number, angle: number) => ({ number, name: "~", electrical_type: "passive", shape: "line", at: [0, y], angle_deg: angle, length_mm: 2.54, unit: 1, body_style: 1, hidden: false });
  const r = { id: "R1", reference: "R1", lib_id: "Dev:R", at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [{ number: "1", name: "~", kind: "passive", net: "/IN" }, { number: "2", name: "~", kind: "passive", net: "/OUT" }] };
  const base = {
    lib_symbols: { "Dev:R": { graphics: [], pins: [libPin("1", 3.81, 270), libPin("2", -3.81, 90)], pin_name_offset: 0 } },
    symbols: [r],
    power_symbols: [],
    wires: [{ id: "w1", net: "/IN", pins: [], pts: [[10_000_000, 0], [30_000_000, 0]], bus: false }],
    no_connects: [],
    labels: [] as unknown[],
    texts: [],
    title_block: null,
    bus_entries: [],
    sheets: [{ id: "s1", name: "sub", file: "sub.kicad_sch", at: [0, 0], size: [1, 1], pins: [{ id: "sp", name: "${OP}", shape: "input", at: [20_000_000, 0] }] }],
    sheet_path: [],
  } as unknown as Schematic;
  const tip = pinItems(base).find((p) => p.number === "2")!.tip;
  const sch = { ...base, labels: [{ id: "l1", net: "OUT ${OP}", at: tip, scope: "local", shape: null }, { id: "l2", net: "${OP}", at: [15_000_000, 0], scope: "local", shape: null }] } as unknown as Schematic;
  const pts = new Map<string, number>([["/out", 0.98946], ["/in", 5]]);
  const out = withOpVars(sch, null, pts);
  assert.equal(out.labels[0]!.net, "OUT 989mV");
  assert.equal(out.labels[1]!.net, "5.00V", "on the wire between its ends");
  assert.equal(out.sheets[0]!.pins[0]!.name, "5.00V");
  assert.equal(netAt(sch, [99, 99]), null);
});

test("r3 S3-2: an escaped \\${OP} that the server's shown text has already undone stays as written", () => {
  const r = { id: "R1", reference: "R1", lib_id: "Dev:R", at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, pins: [], fields: [{ name: "Note", text: "\\${OP}", shown: "${OP}", at: [0, 0], vertical: false, h: "center", v: "center" }, { name: "I", text: "${OP}", shown: "${OP}", at: [0, 0], vertical: false, h: "center", v: "center" }] };
  const sch = { lib_symbols: {}, symbols: [r], power_symbols: [], wires: [], no_connects: [], labels: [], texts: [], title_block: null, bus_entries: [], sheets: [], sheet_path: [] } as unknown as Schematic;
  const out = withOpVars(sch, null, points);
  assert.equal(out.symbols[0]!.fields![0]!.shown, "${OP}");
  assert.equal(out.symbols[0]!.fields![1]!.shown, "1.20mA");
});

test("r3 S3-3: ${OP:<pin>} of a symbol not in the circuit is \"--\" before any run (the model is built anyway), \"?\" for a pin it lacks", () => {
  const pins = [{ number: "1", name: "~" }, { number: "2", name: "~" }];
  assert.equal(symbolOpToken("OP:1", { points: new Map(), ref: "R1", item: undefined, pins }), "--");
  assert.equal(symbolOpToken("OP:7", { points: new Map(), ref: "R1", item: undefined, pins }), "?");
  assert.equal(symbolOpToken("OP:1", { points, ref: "R1", item: undefined, pins }), "1.20mA", "a two-pin model: the item's current");
});

// r4 S4-1: a label on nothing is still a subgraph of its own (`Connection()`), merged with the other labels of its name (`m_local_label_cache[( sheet, name )]`,
// `m_global_label_cache[name]`); alone, its net is its own name.
test("withOpVars: a label on nothing takes the net its name joins", () => {
  const field = (text: string) => ({ name: "OP", text, shown: text, visible: true, italic: false, intersheet_refs: false, at: [0, 0], vertical: false, h: "left", size_nm: 1_270_000 });
  const sch = {
    lib_symbols: {},
    symbols: [],
    power_symbols: [],
    wires: [{ id: "w1", net: "/VIN", pins: [], pts: [[10_000_000, 0], [30_000_000, 0]], bus: false }],
    no_connects: [],
    labels: [
      { id: "a", net: "VIN", at: [0, 50_000_000], scope: "local", shape: null, fields: [field("${OP}")] },
      { id: "b", net: "VIN", at: [20_000_000, 0], scope: "local", shape: null },
      { id: "c", net: "LONE", at: [0, 90_000_000], scope: "local", shape: null, fields: [field("${OP}")] },
      { id: "d", net: "G", at: [0, 70_000_000], scope: "global", shape: null, fields: [field("${OP}")] },
    ],
    texts: [],
    title_block: null,
    bus_entries: [],
    sheets: [],
    sheet_path: [{ id: "s1", name: "sub" }],
  } as unknown as Schematic;
  const out = withOpVars(sch, null, new Map([["/vin", 5], ["g", 1]]));
  assert.equal(out.labels[0]!.fields![0]!.shown, "5.00V", "joins the wired VIN label");
  assert.equal(out.labels[2]!.fields![0]!.shown, "?", "a net of its own, /sub/LONE, which the run has not");
  assert.equal(out.labels[3]!.fields![0]!.shown, "1.00V", "a global label's net is its name");
  assert.equal(withOpVars(sch, null, new Map()).labels[2]!.fields![0]!.shown, "--", "it has a connection: \"--\" before a run, not \"?\"");
});
