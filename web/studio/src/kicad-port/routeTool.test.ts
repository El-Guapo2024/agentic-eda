import { test } from "node:test";
import assert from "node:assert/strict";
import { drawStateFromPreview, createRequestGuard, createMoveThrottle, type RouteDrawState } from "./routeTool";
import type { RoutePreview } from "../api/types";

function baseDraw(overrides: Partial<RouteDrawState> = {}): RouteDrawState {
  return { kind: "route", net: "SIG", layer: "F.Cu", width: 200, pts: [[0, 0]], ...overrides };
}

function basePreview(overrides: Partial<RoutePreview> = {}): RoutePreview {
  return { ok: true, net: "SIG", colliding: false, layer: "F.Cu", head: [[0, 0]], runs: [], via: null, snapped_end: null, displaced: [], displaced_vias: [], ...overrides };
}

test("drawStateFromPreview copies the server's head/colliding/via/snapped_end into the draw state", () => {
  const current = baseDraw({ placingVia: true, pendingViaLayer: "B.Cu" });
  const preview = basePreview({ head: [[0, 0], [1000, 0], [1000, 1000]], colliding: true, via: { x: 1000, y: 1000, diameter: 600, drill: 300 }, snapped_end: [1000, 1000] });
  const next = drawStateFromPreview(current, preview);
  assert.deepEqual(next.pts, preview.head);
  assert.equal(next.colliding, true);
  assert.deepEqual(next.via, preview.via);
  assert.deepEqual(next.snappedEnd, preview.snapped_end);
  // Session-local fields the server never echoes back must survive.
  assert.equal(next.net, "SIG");
  assert.equal(next.placingVia, true);
  assert.equal(next.pendingViaLayer, "B.Cu");
});

test("drawStateFromPreview switches layer when the server reports a via/layer switch", () => {
  const current = baseDraw({ layer: "F.Cu" });
  const preview = basePreview({ layer: "B.Cu", head: [[500, 500]] });
  const next = drawStateFromPreview(current, preview);
  assert.equal(next.layer, "B.Cu");
});

test("drawStateFromPreview carries runs and displaced lines through untouched", () => {
  const current = baseDraw();
  const preview = basePreview({
    runs: [{ layer: "F.Cu", pts: [[0, 0], [1000, 0]] }],
    displaced: [{ source_track: "trk_abc", layer: "F.Cu", pts: [[500, -500], [500, 500]] }],
  });
  const next = drawStateFromPreview(current, preview);
  assert.deepEqual(next.runs, preview.runs);
  assert.deepEqual(next.displaced, preview.displaced);
});

test("drawStateFromPreview carries displaced vias through untouched", () => {
  const current = baseDraw();
  const preview = basePreview({ displaced_vias: [{ source_via: "via_abc", x: 1000, y: 2000 }] });
  const next = drawStateFromPreview(current, preview);
  assert.deepEqual(next.displacedVias, preview.displaced_vias);
});

test("createRequestGuard: only the most recently issued token is current", () => {
  const guard = createRequestGuard();
  const a = guard.next();
  const b = guard.next();
  assert.equal(guard.isCurrent(a), false, "a's reply arrived after b was issued -- stale");
  assert.equal(guard.isCurrent(b), true);
  const c = guard.next();
  assert.equal(guard.isCurrent(b), false, "b is now stale too");
  assert.equal(guard.isCurrent(c), true);
});

test("createRequestGuard: a single in-flight request stays current until superseded", () => {
  const guard = createRequestGuard();
  const token = guard.next();
  assert.equal(guard.isCurrent(token), true);
  assert.equal(guard.isCurrent(token), true, "checking twice doesn't consume it");
});

test("createMoveThrottle: allows the first call, then gates until the interval elapses", () => {
  const throttle = createMoveThrottle(16);
  assert.equal(throttle.shouldSend(0), true);
  assert.equal(throttle.shouldSend(5), false);
  assert.equal(throttle.shouldSend(15), false);
  assert.equal(throttle.shouldSend(16), true);
  assert.equal(throttle.shouldSend(20), false);
  assert.equal(throttle.shouldSend(32), true);
});
