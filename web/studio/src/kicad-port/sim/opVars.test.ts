import { test } from "node:test";
import assert from "node:assert/strict";
import { expandOpVars, labelOpToken, symbolOpToken } from "./opVars";
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
