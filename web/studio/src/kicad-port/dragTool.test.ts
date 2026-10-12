import { test } from "node:test";
import assert from "node:assert/strict";
import { dragStateFromPreview, type DragDrawState } from "./dragTool";
import type { DragPreview } from "../api/types";

function baseDraw(overrides: Partial<DragDrawState> = {}): DragDrawState {
  return { kind: "drag", dragKind: "corner", net: "SIG", layer: "F.Cu", width: 200, pts: [[0, 0]], ...overrides };
}

function basePreview(overrides: Partial<DragPreview> = {}): DragPreview {
  return { ok: true, colliding: false, pts: [[0, 0]], displaced: [], displaced_vias: [], fanout: [], ...overrides };
}

test("dragStateFromPreview copies the server's pts/colliding into the draw state", () => {
  const current = baseDraw();
  const preview = basePreview({ pts: [[0, 0], [1000, 1000]], colliding: true });
  const next = dragStateFromPreview(current, preview);
  assert.deepEqual(next.pts, preview.pts);
  assert.equal(next.colliding, true);
  // Session-local fields the server never echoes back must survive.
  assert.equal(next.net, "SIG");
  assert.equal(next.dragKind, "corner");
  assert.equal(next.layer, "F.Cu");
  assert.equal(next.width, 200);
});

test("dragStateFromPreview carries displaced lines/vias and via fanout through untouched", () => {
  const current = baseDraw({ dragKind: "via", viaDiameter: 600 });
  const preview = basePreview({
    pts: [[2000, 2000]],
    displaced: [{ source_track: "trk_abc", layer: "F.Cu", pts: [[500, -500], [500, 500]] }],
    displaced_vias: [{ source_via: "via_xyz", x: 3000, y: 3000 }],
    fanout: [{ layer: "F.Cu", width: 250, pts: [[0, 0], [2000, 2000]] }],
  });
  const next = dragStateFromPreview(current, preview);
  assert.deepEqual(next.displaced, preview.displaced);
  assert.deepEqual(next.displacedVias, preview.displaced_vias);
  assert.deepEqual(next.fanout, preview.fanout);
  // The via's own size, captured at drag-start, is untouched by a preview reply.
  assert.equal(next.viaDiameter, 600);
});

test("a position the drag cannot resolve shows the last valid line, flagged; a drag that only marks obstacles says so", () => {
  const slid = baseDraw({ dragKind: "segment", pts: [[0, 0], [0, 800], [4000, 800], [4000, 0]] });
  const restored = dragStateFromPreview(slid, basePreview({ drag: "segment", pts: [[0, 0], [0, 500], [4000, 500], [4000, 0]], restored: true }));
  assert.equal(restored.restored, true);
  assert.equal(restored.forcedMark, false);
  assert.equal(restored.dragKind, "segment", "what was grabbed is the session's, captured at drag start");
  const marking = dragStateFromPreview(slid, basePreview({ colliding: true, forced_mark_obstacles: true }));
  assert.equal(marking.forcedMark, true);
  assert.equal(marking.restored, false);
  // a legal position clears both flags again
  const legal = dragStateFromPreview({ ...marking, restored: true }, basePreview());
  assert.equal(legal.restored, false);
});
