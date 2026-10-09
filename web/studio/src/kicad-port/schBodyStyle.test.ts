import { test } from "node:test";
import assert from "node:assert/strict";
import { bodyStyleCount, cycleBodyStyleCmd, nextBodyStyle, setBodyStyleCmd, singleMultiBodyStyleSymbol } from "./schBodyStyle";
import { schContextMenu } from "./schContextMenu";
import { emptySummary } from "./schContextMenu";
import type { LibSymbol, Schematic, SchematicSymbol } from "../api/types";

const lib = (n: number): LibSymbol => ({ graphics: [], pins: [], body_style_count: n });
const sym = (id: string, lib_id: string | null, body_style = 1): SchematicSymbol =>
  ({ id, lib_id, at: [0, 0], rot: 0, mirror: null, unit: 1, body_style, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] }) as unknown as SchematicSymbol;
const sch = {
  symbols: [sym("U1", "74xx:74LS00"), sym("U2", "74xx:74LS00", 2), sym("R1", "Device:R"), sym("X1", null)],
  lib_symbols: { "74xx:74LS00": lib(2), "Device:R": lib(1) },
} as unknown as Pick<Schematic, "symbols" | "lib_symbols">;

test("a library symbol with a De Morgan body has two body styles, any other one", () => {
  assert.equal(bodyStyleCount(sch, sch.symbols[0]!), 2);
  assert.equal(bodyStyleCount(sch, sch.symbols[2]!), 1);
  assert.equal(bodyStyleCount(sch, sch.symbols[3]!), 1, "no library symbol");
  assert.equal(bodyStyleCount({ lib_symbols: { A: { graphics: [], pins: [] } } }, { lib_id: "A" }), 1, "a server that does not say");
});

test("Cycle Body Style goes to the next style and back to the first", () => {
  assert.equal(nextBodyStyle(sch, sch.symbols[0]!), 2);
  assert.equal(nextBodyStyle(sch, sch.symbols[1]!), 1);
  assert.equal(nextBodyStyle(sch, sch.symbols[2]!), 1, "a one-style symbol stays in its style");
});

test("the context menu's condition is one selected symbol with more than one body style", () => {
  assert.equal(singleMultiBodyStyleSymbol(sch, ["U1"]), true);
  assert.equal(singleMultiBodyStyleSymbol(sch, ["U1", "U2"]), false);
  assert.equal(singleMultiBodyStyleSymbol(sch, ["R1"]), false);
  assert.equal(singleMultiBodyStyleSymbol(sch, ["wire_1"]), false);
});

test("Cycle Body Style on a selection takes the first symbol with another body style", () => {
  const cmd = cycleBodyStyleCmd(sch, ["R1", "U2", "U1"]) as unknown as { verb: string; ids: string[]; style?: number };
  assert.equal(cmd.verb, "set_body_style");
  assert.deepEqual(cmd.ids, ["U2"]);
  assert.equal(cmd.style, undefined, "the next style, whichever that is");
  assert.equal(cycleBodyStyleCmd(sch, ["R1"]), null);
  assert.equal(cycleBodyStyleCmd(sch, []), null);
});

test("selecting a body style takes each symbol that has one, once", () => {
  const cmd = setBodyStyleCmd(sch, ["U1", "R1", "U2", "U1"], 2) as unknown as { ids: string[]; style: number };
  assert.deepEqual(cmd.ids, ["U1", "U2"]);
  assert.equal(cmd.style, 2);
  assert.equal(setBodyStyleCmd(sch, ["R1"], 2), null);
});

test("the symbol menu offers Cycle Body Style for one multi-body-style symbol only", () => {
  const flat = (s: ReturnType<typeof emptySummary>) => JSON.stringify(schContextMenu(s));
  const one = { ...emptySummary(), total: 1, symbols: 1, unlocked: 1 };
  assert.ok(!flat(one).includes("toggleDeMorgan"), "a symbol with one body style");
  assert.ok(flat({ ...one, multiBodyStyle: true }).includes("eeschema.InteractiveEdit.toggleDeMorgan"));
  assert.ok(!flat({ ...one, total: 2, symbols: 2, multiBodyStyle: true }).includes("toggleDeMorgan"), "two symbols");
});
