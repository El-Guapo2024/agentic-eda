import { test } from "node:test";
import assert from "node:assert/strict";
import { addLayerPair, cycleLayerPairPreset, defaultLayerPairSettings, enabledLayerPairs, hasSameLayers, otherLayerOfPair, removeLayerPair, sanitizeLayerPair, setCurrentLayerPair, setLayerPairs, type LayerPairInfo } from "./layerPairs";

const CU4 = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
const info = (a: string, b: string, enabled = true, name: string | null = null): LayerPairInfo => ({ pair: { a, b }, enabled, name });

test("the default pair is the outer copper layers with no presets", () => {
  const s = defaultLayerPairSettings(CU4);
  assert.deepEqual(s.current, { a: "F.Cu", b: "B.Cu" });
  assert.equal(s.pairs.length, 0);
  assert.equal(s.lastManual, null);
});

test("HasSameLayers ignores the order", () => {
  assert.ok(hasSameLayers({ a: "F.Cu", b: "B.Cu" }, { a: "B.Cu", b: "F.Cu" }));
  assert.ok(!hasSameLayers({ a: "F.Cu", b: "B.Cu" }, { a: "F.Cu", b: "In1.Cu" }));
});

test("AddLayerPair refuses a pair with the same layers, even reversed", () => {
  let s = defaultLayerPairSettings(CU4);
  let r = addLayerPair(s, info("F.Cu", "In1.Cu"));
  assert.ok(r.added);
  s = r.settings;
  r = addLayerPair(s, info("In1.Cu", "F.Cu"));
  assert.ok(!r.added);
  assert.equal(r.settings.pairs.length, 1);
});

test("RemoveLayerPair removes the matching stored pair", () => {
  let s = addLayerPair(defaultLayerPairSettings(CU4), info("F.Cu", "In1.Cu")).settings;
  s = addLayerPair(s, info("In2.Cu", "B.Cu")).settings;
  const r = removeLayerPair(s, { a: "B.Cu", b: "In2.Cu" });
  assert.ok(r.removed);
  assert.deepEqual(
    r.settings.pairs.map((p) => p.pair),
    [{ a: "F.Cu", b: "In1.Cu" }]
  );
  assert.ok(!removeLayerPair(r.settings, { a: "x", b: "y" }).removed);
});

test("SetLayerPairs replaces the list and skips duplicates", () => {
  const s = setLayerPairs(defaultLayerPairSettings(CU4), [info("F.Cu", "B.Cu"), info("B.Cu", "F.Cu"), info("In1.Cu", "In2.Cu")]);
  assert.equal(s.pairs.length, 2);
});

test("SetCurrentLayerPair: a pair that is not an enabled preset becomes the manual pair", () => {
  let s = setLayerPairs(defaultLayerPairSettings(CU4), [info("F.Cu", "B.Cu"), info("In1.Cu", "In2.Cu", false)]);
  s = setCurrentLayerPair(s, { a: "F.Cu", b: "B.Cu" });
  assert.equal(s.lastManual, null);
  s = setCurrentLayerPair(s, { a: "In1.Cu", b: "In2.Cu" }); // preset, but disabled
  assert.deepEqual(s.lastManual, { a: "In1.Cu", b: "In2.Cu" });
  s = setCurrentLayerPair(s, { a: "F.Cu", b: "In2.Cu" });
  assert.deepEqual(s.lastManual, { a: "F.Cu", b: "In2.Cu" });
});

test("adding the manual pair as a preset drops the manual entry", () => {
  let s = setCurrentLayerPair(defaultLayerPairSettings(CU4), { a: "F.Cu", b: "In1.Cu" });
  assert.deepEqual(s.lastManual, { a: "F.Cu", b: "In1.Cu" });
  s = addLayerPair(s, info("In1.Cu", "F.Cu")).settings;
  assert.equal(s.lastManual, null);
});

test("GetEnabledLayerPairs lists the manual pair first, then enabled presets, and finds the current one", () => {
  let s = setLayerPairs(defaultLayerPairSettings(CU4), [info("F.Cu", "B.Cu", true, "outer"), info("In1.Cu", "In2.Cu", false), info("F.Cu", "In1.Cu")]);
  s = setCurrentLayerPair(s, { a: "In1.Cu", b: "B.Cu" });
  const e = enabledLayerPairs(s);
  assert.deepEqual(
    e.pairs.map((p) => p.name ?? `${p.pair.a}/${p.pair.b}`),
    ["Manual", "outer", "F.Cu/In1.Cu"]
  );
  assert.equal(e.current, 0);
  assert.equal(enabledLayerPairs(setCurrentLayerPair(s, { a: "In1.Cu", b: "F.Cu" })).current, 2);
});

test("CycleLayerPresets needs two pairs and wraps around", () => {
  const one = setLayerPairs(defaultLayerPairSettings(CU4), [info("F.Cu", "B.Cu")]);
  assert.equal(cycleLayerPairPreset(one), null);
  let s = setLayerPairs(defaultLayerPairSettings(CU4), [info("F.Cu", "B.Cu"), info("F.Cu", "In1.Cu"), info("In2.Cu", "B.Cu")]);
  s = setCurrentLayerPair(s, { a: "F.Cu", b: "B.Cu" });
  const seen: string[] = [];
  for (let i = 0; i < 4; i++) {
    s = cycleLayerPairPreset(s)!;
    seen.push(`${s.current.a}/${s.current.b}`);
  }
  assert.deepEqual(seen, ["F.Cu/In1.Cu", "In2.Cu/B.Cu", "F.Cu/B.Cu", "F.Cu/In1.Cu"]);
});

test("LayerToggle goes top to bottom and from any other layer to the top", () => {
  const pair = { a: "F.Cu", b: "In2.Cu" };
  assert.equal(otherLayerOfPair(pair, "F.Cu"), "In2.Cu");
  assert.equal(otherLayerOfPair(pair, "In2.Cu"), "F.Cu");
  assert.equal(otherLayerOfPair(pair, "B.Cu"), "F.Cu");
});

test("a pair naming a layer the board no longer has falls back to the outer pair", () => {
  let s = setLayerPairs(defaultLayerPairSettings(CU4), [info("F.Cu", "In1.Cu"), info("In2.Cu", "B.Cu")]);
  s = setCurrentLayerPair(s, { a: "In1.Cu", b: "In2.Cu" });
  const two = sanitizeLayerPair(s, ["F.Cu", "B.Cu"]);
  assert.equal(two.pairs.length, 0);
  assert.deepEqual(two.current, { a: "F.Cu", b: "B.Cu" });
  assert.equal(two.lastManual, null);
  assert.equal(sanitizeLayerPair(defaultLayerPairSettings(CU4), CU4).pairs.length, 0);
});
