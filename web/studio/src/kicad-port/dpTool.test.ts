import { test } from "node:test";
import assert from "node:assert/strict";
import { dpStateFromPreview, dpViaRequest, type DpDrawState } from "./dpTool";
import type { DiffPairPreview } from "../api/types";

function baseDraw(overrides: Partial<DpDrawState> = {}): DpDrawState {
  return { kind: "diffpair", netA: "USB_DP", netB: "USB_DN", layer: "F.Cu", width: 125, ptsA: [[0, 0]], ptsB: [[0, 300]], ...overrides };
}

function basePreview(overrides: Partial<DiffPairPreview> = {}): DiffPairPreview {
  return { ok: true, net_a: "USB_DP", net_b: "USB_DN", layer: "F.Cu", width: 125, colliding: false, head_a: [[0, 0]], head_b: [[0, 300]], runs_a: [], runs_b: [], snapped_end: false, ...overrides };
}

test("dpStateFromPreview copies the server's heads/colliding/snapped_end into the draw state", () => {
  const current = baseDraw();
  const preview = basePreview({ head_a: [[0, 0], [1000, 0]], head_b: [[0, 300], [1000, 300]], colliding: true, snapped_end: true });
  const next = dpStateFromPreview(current, preview);
  assert.deepEqual(next.ptsA, preview.head_a);
  assert.deepEqual(next.ptsB, preview.head_b);
  assert.equal(next.colliding, true);
  assert.equal(next.snappedEnd, true);
  // Session-local fields the server already echoes back, but which must
  // still survive a reply that happens to omit them (net names are
  // optional on the wire, see RoutePreview's own net field for precedent).
  assert.equal(next.netA, "USB_DP");
  assert.equal(next.netB, "USB_DN");
});

test("dpStateFromPreview carries fixed runs for both lines through untouched", () => {
  const current = baseDraw();
  const preview = basePreview({
    runs_a: [{ layer: "F.Cu", pts: [[0, 0], [1000, 0]] }],
    runs_b: [{ layer: "F.Cu", pts: [[0, 300], [1000, 300]] }],
  });
  const next = dpStateFromPreview(current, preview);
  assert.deepEqual(next.runsA, preview.runs_a);
  assert.deepEqual(next.runsB, preview.runs_b);
});

test("dpStateFromPreview keeps the previous net names when a reply has none", () => {
  const current = baseDraw({ netA: "USB_DP", netB: "USB_DN" });
  const preview = basePreview({ net_a: null, net_b: null });
  const next = dpStateFromPreview(current, preview);
  assert.equal(next.netA, "USB_DP");
  assert.equal(next.netB, "USB_DN");
});

test("dpStateFromPreview carries the pair of vias, the fixed legs' vias and what a shove would push aside", () => {
  const via = (net: string, x: number, y: number) => ({ net, x, y, diameter: 600, drill: 300, from_layer: "F.Cu", to_layer: "B.Cu" });
  const preview = basePreview({
    gap: 250,
    placing_via: true,
    vias: [via("USB_DP", 5000, 75), via("USB_DN", 5000, 925)],
    fixed_vias: [via("USB_DP", 1000, 0)],
    displaced: [{ source_track: "u", layer: "F.Cu", pts: [[3000, -1500], [5000, -300], [7000, -1500]] }],
    displaced_vias: [{ source_via: "v1", x: 2000, y: 2000 }],
  });
  const next = dpStateFromPreview(baseDraw(), preview);
  assert.equal(next.gap, 250);
  assert.equal(next.placingVia, true);
  assert.deepEqual(next.vias, [
    { net: "USB_DP", x: 5000, y: 75, diameter: 600, fromLayer: "F.Cu", toLayer: "B.Cu" },
    { net: "USB_DN", x: 5000, y: 925, diameter: 600, fromLayer: "F.Cu", toLayer: "B.Cu" },
  ]);
  assert.equal(next.fixedVias?.length, 1);
  assert.deepEqual(next.displaced, preview.displaced);
  assert.deepEqual(next.displacedVias, preview.displaced_vias);
  // a later reply without them clears them (the head moved off the obstacle, V was turned off)
  const cleared = dpStateFromPreview(next, basePreview());
  assert.equal(cleared.placingVia, false);
  assert.deepEqual(cleared.vias, []);
  assert.deepEqual(cleared.displaced, []);
});

test("dpViaRequest arms the vias toward the given layer, and a second V takes them away", () => {
  assert.deepEqual(dpViaRequest(baseDraw(), "B.Cu"), { enabled: true, toLayer: "B.Cu" });
  assert.deepEqual(dpViaRequest(baseDraw({ placingVia: true }), "B.Cu"), { enabled: false, toLayer: "B.Cu" });
});

test("a_pair_via_takes_the_chosen_via_size (R7-R2: handleLayerSwitch's GetCurrentViaSize)", () => {
  assert.deepEqual(dpViaRequest(baseDraw(), "B.Cu", { diameter: 800_000, drill: 400_000 }), { enabled: true, toLayer: "B.Cu", diameter: 800_000, drill: 400_000 });
  assert.deepEqual(dpViaRequest(baseDraw(), "B.Cu", undefined), { enabled: true, toLayer: "B.Cu" }, "the net class's: the server's rules");
  assert.deepEqual(dpViaRequest(baseDraw({ placingVia: true }), "B.Cu", { diameter: 800_000, drill: 400_000 }), { enabled: false, toLayer: "B.Cu" });
});
