import { test } from "node:test";
import assert from "node:assert/strict";
import { dpStateFromPreview, type DpDrawState } from "./dpTool";
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
