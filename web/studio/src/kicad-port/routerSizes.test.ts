import { test } from "node:test";
import assert from "node:assert/strict";
import { chosenDiffPair, chosenTrackWidthOf, diffPairLabel, routeWidthAfterChange, routerMessagePanel, stepDiffPairIndex, stepTrackWidth, comboBoxUnits, sizeBoxPick, trackWidthBoxLabels, viaSizeBoxLabels, withEventSizes, type RouteSizesInfo } from "./routerSizes";

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

// UI-LEFT R7: the auxiliary toolbar's Track Width and Via Size boxes (`UpdateTrackWidthSelectBox`, `UpdateViaSizeSelectBox`, `Tracks_and_Vias_Size_Event`).
test("the toolbar's width and via boxes list KiCad's entries", () => {
  assert.equal(comboBoxUnits("mm", 250_000), "0.250 mm");
  assert.equal(comboBoxUnits("mil", 254_000), "10.00 mils");
  assert.equal(comboBoxUnits("in", 254_000, false), "0.01000");
  assert.deepEqual(trackWidthBoxLabels([200_000, 254_000], "mm"), ["Track: use netclass width", "Track: 0.200 mm (7.87 mils)", "Track: 0.254 mm (10.00 mils)", "---", "Edit Pre-defined Sizes..."]);
  assert.deepEqual(viaSizeBoxLabels([{ diameter: 600_000, drill: 300_000 }, { diameter: 500_000, drill: 0 }], "mil"), [
    "Via: use netclass sizes",
    "Via: 23.62 / 11.81 mils (0.600 / 0.300 mm)",
    "Via: 19.69 mils (0.500 mm)",
    "---",
    "Edit Pre-defined Sizes...",
  ]);
});

test("a pick in a toolbar box is the list's index; the separator does nothing and the last entry edits the sizes", () => {
  assert.deepEqual(sizeBoxPick(0, 5), { kind: "index", index: 0 });
  assert.deepEqual(sizeBoxPick(2, 5), { kind: "index", index: 2 });
  assert.deepEqual(sizeBoxPick(3, 5), { kind: "none" });
  assert.deepEqual(sizeBoxPick(4, 5), { kind: "edit" });
});

// UI-LEFT R5: what `updateSizesAfterRouterEvent` left, folded into the draw state.
test("the sizes asked again after a layer switch reach the message panel", () => {
  const draw: { width: number; gap?: number; sizes?: RouteSizesInfo } = { width: 200_000, sizes: { width_source: "existing track", clearance: 200_000, clearance_source: "netclass 'Default'" } };
  const kept = withEventSizes(draw, { clearance: 500_000, clearance_source: "bclr", width: 350_000, width_source: "bwidth", width_applied: false });
  assert.equal(kept.width, 200_000, "the head keeps its width");
  assert.deepEqual(kept.sizes, { width: 350_000, width_source: "bwidth", clearance: 500_000, clearance_source: "bclr" });
  const items = routerMessagePanel({ pair: false, net: "SIG", width: kept.width }, kept.sizes, { mode: "walkaround", freeAngle: false }, (nm) => `${nm / 1e6} mm`);
  assert.deepEqual(items.find((i) => i.upper.startsWith("Track Width")), { upper: "Track Width: 0.35 mm", lower: "(from bwidth)" }, "KiCad's panel shows `sizes.TrackWidth()`");
  assert.deepEqual(items.find((i) => i.upper.startsWith("Min Clearance")), { upper: "Min Clearance: 0.5 mm", lower: "(from bclr)" });
  const taken = withEventSizes(draw, { clearance: 200_000, clearance_source: "netclass 'Default'", width: 350_000, width_source: "bwidth", width_applied: true });
  assert.equal(taken.width, 350_000);
  assert.equal(taken.sizes?.width, null);
  assert.equal(withEventSizes(draw, null), draw);
});
