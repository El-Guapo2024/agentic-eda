import { test } from "node:test";
import assert from "node:assert/strict";
import { filterFootprints, footprintFilterMatch, splitFootprintId, toCandidate, uniquePadCount, uniquePinCount, wildcardAnchoredMatch } from "./footprintFilter";

const list = [
  toCandidate("Resistor_SMD:R_0402_1005Metric", 2),
  toCandidate("Resistor_SMD:R_0603_1608Metric", 2),
  toCandidate("Capacitor_SMD:C_0603_1608Metric", 2),
  toCandidate("Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", 8),
  toCandidate("SOT-23", null),
];

test("an id splits into library and name", () => {
  assert.deepEqual(splitFootprintId("Lib:Name:x"), { lib: "Lib", name: "Name:x" });
  assert.deepEqual(splitFootprintId("SOT-23"), { lib: "", name: "SOT-23" });
});

test("wildcards are anchored: the whole name must match", () => {
  assert.equal(wildcardAnchoredMatch("r_*", "r_0603_1608metric"), true);
  assert.equal(wildcardAnchoredMatch("r_*", "xr_0603"), false);
  assert.equal(wildcardAnchoredMatch("sot-23?", "sot-23w"), true);
  assert.equal(wildcardAnchoredMatch("sot-23?", "sot-23"), false);
  assert.equal(wildcardAnchoredMatch("c.0603", "c.0603"), true, "a dot is a dot, not a wildcard");
  assert.equal(wildcardAnchoredMatch("c.0603", "cx0603"), false);
});

test("the symbol's filters match the lower-cased name, or library:name when the filter holds a colon; none passes all", () => {
  const r = toCandidate("Resistor_SMD:R_0603_1608Metric");
  assert.equal(footprintFilterMatch(r, []), true);
  assert.equal(footprintFilterMatch(r, ["R_*"]), true);
  assert.equal(footprintFilterMatch(r, ["C_*", "SOT*"]), false);
  assert.equal(footprintFilterMatch(r, ["Resistor_SMD:R_*"]), true);
  assert.equal(footprintFilterMatch(r, ["Capacitor_SMD:R_*"]), false);
});

test("the filters combine: pin count, library, the symbol's filters and the words typed", () => {
  const ids = (f: Parameters<typeof filterFootprints>[1]) => filterFootprints(list, f).map((c) => c.id);
  assert.equal(ids({}).length, 5);
  assert.deepEqual(ids({ pinCount: 8 }), ["Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"]);
  assert.deepEqual(ids({ pinCount: 2, library: "Resistor_SMD" }), ["Resistor_SMD:R_0402_1005Metric", "Resistor_SMD:R_0603_1608Metric"]);
  assert.deepEqual(ids({ symbolFilters: ["*0603*"] }), ["Resistor_SMD:R_0603_1608Metric", "Capacitor_SMD:C_0603_1608Metric"]);
  assert.deepEqual(ids({ text: "0603 cap" }), ["Capacitor_SMD:C_0603_1608Metric"]);
  assert.deepEqual(ids({ text: "  " }).length, 5);
  assert.deepEqual(ids({ pinCount: 3 }), [], "no pad count matches");
});

test("pads count by different numbers; unnumbered pads and non-plated holes do not count", () => {
  assert.equal(
    uniquePadCount([
      { number: "1", kind: "smd" },
      { number: "1", kind: "smd" },
      { number: "2", kind: "smd" },
      { number: "", kind: "smd" },
      { number: "3", kind: "non_plated_hole" },
    ]),
    2
  );
  assert.equal(uniquePinCount([{ number: "[1-3]" }, { number: "3" }, { number: "4" }], (n) => (n === "[1-3]" ? ["1", "2", "3"] : [n])), 4);
});
