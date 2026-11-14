import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardViaStack } from "../api/pcbItemTypes";
import type { ViaStackPreset } from "../api/types";
import { buildMembers, viaStackTargetLayer, hopPos, isSpanValid, layerRange, newViaStack, placementPitch, placementPreset, presetOf, steeredHop, viaStackBoxLabels, viaStackBoxPick, viaStackCmd, withPreset } from "./viaStack";
import { isViaDrcMessage, isViaPlacement, viaDrcInfoBarText } from "./viaDrcInfoBar";

const copper = ["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"];
const sizes = { current: { diameter: 600_000, drill: 300_000 }, netclass: { size: 300_000, drill: 150_000 }, traceWidth: 100_000 };
const hdi: ViaStackPreset = { name: "HDI", start_layer: "F.Cu", end_layer: "In2.Cu", staggered: false, via_size_nm: 250_000, via_drill_nm: 100_000, use_netclass: false, filled: true, capped: true, pitch_nm: 0 };

test("UI-LEFT-3 X3-1: a span walks the copper in stack order either way (LAYER_RANGE), IsSpanValid", () => {
  assert.deepEqual(layerRange(copper, "F.Cu", "In2.Cu"), ["F.Cu", "In1.Cu", "In2.Cu"]);
  assert.deepEqual(layerRange(copper, "B.Cu", "In3.Cu"), ["B.Cu", "In4.Cu", "In3.Cu"]);
  assert.equal(isSpanValid(copper, "F.Cu", "In9.Cu"), false);
  assert.equal(isSpanValid(copper, "In1.Cu", "In1.Cu"), false);
  assert.equal(isSpanValid(copper, "In1.Cu", "B.Cu"), true);
});

test("UI-LEFT-3 X3-1: ApplyPreset / ToPreset, and BuildMembers of a stacked stack (one hop per pair, coaxial, the stack's size)", () => {
  const s = withPreset(newViaStack({ x: 10, y: 20 }), hdi);
  assert.deepEqual(presetOf(s), hdi);
  const m = buildMembers(s, copper, sizes);
  assert.deepEqual(
    m.map((x) => (x.kind === "via" ? [x.at, x.top, x.bottom, x.size, x.drill] : null)),
    [
      [[10, 20], "F.Cu", "In1.Cu", 250_000, 100_000],
      [[10, 20], "In1.Cu", "In2.Cu", 250_000, 100_000],
    ]
  );
  // "0 means take from netclass / board defaults"; with "Use netclass values" the class's microvia sizes
  assert.equal((buildMembers({ ...s, via_size_nm: 0, via_drill_nm: 0 }, copper, sizes)[0] as { size: number }).size, 600_000);
  assert.equal((buildMembers({ ...s, use_netclass: true }, copper, sizes)[0] as { size: number }).size, 300_000);
  assert.deepEqual(buildMembers({ ...s, end_layer: "In9.Cu" }, copper, sizes), []);
});

test("UI-LEFT-3 X3-1: a staggered stack's hops (m_hops, then on along the last step by the pitch, else along X) and its traces on the landing layers", () => {
  const s: BoardViaStack = { ...newViaStack({ x: 0, y: 0 }), start_layer: "B.Cu", end_layer: "In2.Cu", style: "staggered", pitch_nm: 500, hops: [{ x: 0, y: 0 }, { x: 0, y: 500 }] };
  assert.deepEqual([0, 1, 2].map((i) => hopPos(s, i)), [[0, 0], [0, 500], [0, 1000]]);
  assert.deepEqual(hopPos({ ...s, hops: undefined }, 2), [1000, 0]);
  const m = buildMembers(s, copper, sizes);
  const vias = m.filter((x) => x.kind === "via");
  const traces = m.filter((x) => x.kind === "trace");
  assert.deepEqual(vias.map((v) => (v.kind === "via" ? [v.top, v.bottom] : [])), [["In4.Cu", "B.Cu"], ["In3.Cu", "In4.Cu"], ["In2.Cu", "In3.Cu"]]);
  assert.deepEqual(traces.map((t) => (t.kind === "trace" ? [t.layer, t.a, t.b, t.width] : [])), [["In4.Cu", [0, 0], [0, 500], 100_000], ["In3.Cu", [0, 500], [0, 1000], 100_000]]);
});

test("UI-LEFT-3 X3-1: the placement's preset (clamped index, none without presets), its pitch (twice the via when none), the steered hop", () => {
  assert.equal(placementPreset([], 0), null);
  assert.equal(placementPreset([hdi, { ...hdi, name: "B" }], 7)?.name, "B");
  const st = withPreset(newViaStack({ x: 0, y: 0 }), { ...hdi, staggered: true, pitch_nm: 0 });
  assert.equal(placementPitch(st, 600_000), 500_000);
  assert.equal(placementPitch({ ...st, via_size_nm: 0 }, 600_000), 1_200_000);
  assert.equal(placementPitch({ ...st, pitch_nm: 400 }, 600_000), 400);
  // "Cursor is locked onto a circle of radius = pitch around the previous hop", 45 / 90 degree steps unless free or Ctrl
  assert.deepEqual(steeredHop([0, 0], [1000, 100], 500, "deg90", false), [500, 0]);
  assert.deepEqual(steeredHop([0, 0], [1000, 900], 500, "deg45", false), [354, 354]);
  assert.deepEqual(steeredHop([0, 0], [300, 400], 500, "deg45", true), [300, 400]);
});

test("UI-LEFT-3 X3-1: the toolbar's microvia stack selector (UpdateViaStackSelectBox, SelectViaStack_Event)", () => {
  assert.deepEqual(viaStackBoxLabels([]), ["Microvia stack: none defined", "---", "Edit Pre-defined Microvia Stacks..."]);
  assert.deepEqual(viaStackBoxLabels([hdi]), ["Microvia stack: HDI", "---", "Edit Pre-defined Microvia Stacks..."]);
  assert.deepEqual(viaStackBoxPick(0, 4, 2), { kind: "index", index: 0 });
  assert.deepEqual(viaStackBoxPick(2, 4, 2), { kind: "none" });
  assert.deepEqual(viaStackBoxPick(3, 4, 2), { kind: "edit" });
  assert.deepEqual(viaStackBoxPick(0, 3, 0), { kind: "none" });
});

test("UI-LEFT-3 X3-1: set_via_stack leaves the members and the id to the server; the stack's DRC refusal is the info bar's", () => {
  const cmd = viaStackCmd(null, { ...newViaStack({ x: 1, y: 2 }), id: "vsk_1", member_ids: ["a"] }) as unknown as Record<string, unknown>;
  assert.equal(cmd.verb, "set_via_stack");
  assert.equal("id" in cmd, false);
  assert.equal("member_ids" in (cmd.stack as object), false);
  assert.equal(isViaDrcMessage("pcb_item", "ops_via_drc: Microvia stack location violates DRC."), true);
  assert.equal(viaDrcInfoBarText("ops_via_drc: Microvia stack location violates DRC."), "Microvia stack location violates DRC.");
  assert.equal(isViaPlacement(cmd as { op: string; verb?: string }), true);
  assert.equal(isViaPlacement({ op: "pcb_item", verb: "set_via_stack", id: "vsk_1" }), false);
});

test("UI-LEFT-3 X3-1: ViaStackTargetLayer -- from the start layer the end, from the end layer the start, else none", () => {
  assert.equal(viaStackTargetLayer("F.Cu", "In2.Cu", "F.Cu"), "In2.Cu");
  assert.equal(viaStackTargetLayer("F.Cu", "In2.Cu", "In2.Cu"), "F.Cu");
  assert.equal(viaStackTargetLayer("F.Cu", "In2.Cu", "In1.Cu"), null);
  assert.equal(viaStackTargetLayer("F.Cu", "F.Cu", "F.Cu"), null);
});
