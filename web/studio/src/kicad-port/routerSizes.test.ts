import { test } from "node:test";
import assert from "node:assert/strict";
import { chosenDiffPair, chosenTrackWidthOf, diffPairLabel, routeWidthAfterChange, routerMessagePanel, stepDiffPairIndex, stepTrackWidth } from "./routerSizes";

const mm = (nm: number) => `${nm / 1e6} mm`;

// r5 R5-6: `TrackWidthInc` / `TrackWidthDec` step the board's list by index; the first press of a route at its inherited width only overrides it.
test("W steps the board's width list, and the first press of a route that inherited its width goes to the current entry", () => {
  // the list: the class's entry, 0.3, 0.6
  assert.deepEqual(stepTrackWidth(0, 3, 1, { autoTrackWidth: true, override: false }), { index: 0, override: true }, "first press: the current entry");
  assert.deepEqual(stepTrackWidth(0, 3, 1, { autoTrackWidth: true, override: true }), { index: 1, override: true });
  assert.deepEqual(stepTrackWidth(2, 3, 1, { autoTrackWidth: true, override: true }), { index: 0, override: true }, "wraps to the start");
  assert.deepEqual(stepTrackWidth(0, 3, -1, { autoTrackWidth: false, override: false }), { index: 2, override: true }, "Shift+W wraps to the end; with Use Starting Track Width off there is nothing to override");
  assert.deepEqual(stepTrackWidth(1, 3, 1, null), { index: 2, override: false }, "no route: the index steps");
  // what the route then takes: the entry, or the net's rule width at entry 0
  assert.equal(routeWidthAfterChange([300_000, 600_000], 0, false, 0, 150_000), 150_000);
  assert.equal(routeWidthAfterChange([300_000, 600_000], 2, false, 0, 150_000), 600_000);
  assert.equal(routeWidthAfterChange([300_000, 600_000], 2, true, 420_000, 150_000), 420_000, "the custom width while custom sizes are in use");
});

// r5 R5-9: the chosen width is read from the list at route time.
test("the chosen width is the list's entry now, not the value it had when chosen", () => {
  assert.equal(chosenTrackWidthOf([350_000, 600_000], 1, false, 300_000), 350_000);
  assert.equal(chosenTrackWidthOf([350_000], 2, false, 600_000), undefined, "an entry removed since: the net class's");
  assert.equal(chosenTrackWidthOf([350_000], 0, false, null), undefined);
  assert.equal(chosenTrackWidthOf([350_000], 1, true, 420_000), 420_000, "the custom width");
});

// r5 R5-5: `DIFF_PAIR_MENU::update`'s labels, `TrackWidthInc` on the pair list, and the dimensions `ImportSizes` takes from the choice.
test("the pair list's labels, its stepping, and the dimensions a choice gives", () => {
  assert.equal(diffPairLabel({ width: 200_000, gap: 0, via_gap: 0 }, mm), "Width 0.2 mm");
  assert.equal(diffPairLabel({ width: 100_000, gap: 100_000, via_gap: 150_000 }, mm), "Width 0.1 mm, gap 0.1 mm, via gap 0.15 mm");
  assert.equal(diffPairLabel({ width: 100_000, gap: 100_000, via_gap: 0 }, mm), "Width 0.1 mm, gap 0.1 mm");
  assert.equal(diffPairLabel({ width: 100_000, gap: 0, via_gap: 150_000 }, mm), "Width 0.1 mm, via gap 0.15 mm");
  assert.equal(stepDiffPairIndex(0, 3, 1), 1);
  assert.equal(stepDiffPairIndex(2, 3, 1), 0);
  assert.equal(stepDiffPairIndex(0, 3, -1), 2);
  const presets = [{ width: 200_000, gap: 0, via_gap: 0 }, { width: 100_000, gap: 100_000, via_gap: 150_000 }];
  assert.deepEqual(chosenDiffPair(presets, 2, false, null), { width: 100_000, gap: 100_000, viaGap: 150_000 });
  assert.equal(chosenDiffPair(presets, 0, false, null), undefined, "entry 0: the net class's");
  assert.deepEqual(chosenDiffPair(presets, 2, true, { width: 1, gap: 2, viaGap: 3 }), { width: 1, gap: 2, viaGap: 3 }, "custom wins");
});

// r5 R5-8: `ROUTER_TOOL::UpdateMessagePanel`.
test("the router's message panel names the width, the clearance and the gap with where they came from", () => {
  const items = routerMessagePanel({ pair: false, net: "SIG", width: 150_000 }, { width_source: "existing track", clearance: 200_000, clearance_source: "netclass 'Default'", netclass: "Default" }, { mode: "walkaround", freeAngle: false, cornerMode: "rounded_45" }, mm);
  assert.deepEqual(items, [
    { upper: "Routing Track: SIG", lower: "Resolved Netclass: Default" },
    { upper: "Corner Style", lower: "45-degree rounded" },
    { upper: "Mode", lower: "Walk around" },
    { upper: "Track Width: 0.15 mm", lower: "(from existing track)" },
    { upper: "Min Clearance: 0.2 mm", lower: "(from netclass 'Default')" },
  ]);
  const pair = routerMessagePanel({ pair: true, netA: "D+", netB: "D-", width: 200_000, gap: 150_000 }, { width_source: "user choice", gap_source: "user choice", clearance: 200_000, clearance_source: "board minimum clearance", netclass: "usb" }, { mode: "shove", freeAngle: false }, mm);
  assert.equal(pair[0]!.upper, "Routing Diff Pair: D+, D-");
  assert.equal(pair[2]!.lower, "Shove");
  assert.deepEqual(pair[5], { upper: "Diff Pair Gap: 0.15 mm", lower: "(from user choice)" });
  assert.deepEqual(routerMessagePanel({ pair: false, net: null, width: 1 }, undefined, { mode: "mark_obstacles", freeAngle: true }, mm).slice(0, 3), [
    { upper: "Routing Track", lower: "(no net)" },
    { upper: "Corner Style", lower: "Free-angle" },
    { upper: "Mode", lower: "Highlight collisions" },
  ]);
});
