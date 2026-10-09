import { test } from "node:test";
import assert from "node:assert/strict";
import { ConstructionManager, EXTENSION_SNAP_TIMEOUT_MS, MAX_TEMPORARY_BATCHES, SnapLineManager, SnapManager, type ConstructionBatch } from "./constructionManager";
import { buildScene, type SnapItem } from "./snapScene";
import type { Pt } from "./snapGeom";

const seg = (id: string, y: number) => ({ id, kind: "segment" as const, layer: "F.SilkS", stroke_width: 150, filled: false, start: [0, y] as [number, number], end: [1000, y] as [number, number] });
const items: readonly SnapItem[] = buildScene({ parts: [], drawings: { shapes: [seg("a", 0), seg("b", 100), seg("c", 200), seg("d", 300)], texts: [], dimensions: [] } }).items;
const [A, B, C, D] = [items[0]!, items[1]!, items[2]!, items[3]!];
const batch = (...of: SnapItem[]): ConstructionBatch => of.map((item) => ({ source: "items", item, constructions: [{ drawable: { t: "point", p: [item.bbox.x0, item.bbox.y0] }, lineWidth: 1 }] }));
const near = (got: Pt | null, want: Pt, tol = 1e-6) => {
  assert.ok(got, "a point");
  assert.ok(Math.abs(got![0] - want[0]) <= tol && Math.abs(got![1] - want[1]) <= tol, `expected (${want}) got (${got})`);
};

test("a persistent batch is accepted at once, and the next replaces it", () => {
  const m = new ConstructionManager();
  m.proposeConstructionItems(batch(A), true, 0);
  assert.equal(m.hasActiveConstruction(), true);
  assert.equal(m.involvesAllGivenRealItems([A]), true);
  assert.equal(m.involvesAllGivenRealItems([B]), false);
  m.proposeConstructionItems(batch(B), true, 10);
  assert.equal(m.involvesAllGivenRealItems([A]), false, "only one persistent batch is kept");
  assert.equal(m.involvesAllGivenRealItems([B]), true);
});

test("an empty batch is not worth proposing", () => {
  const m = new ConstructionManager();
  m.proposeConstructionItems([], false, 0);
  assert.equal(m.hasActiveConstruction(), false);
});

test("the first two temporary batches are accepted at once, a third waits its timeout and pushes the oldest out", () => {
  assert.equal(MAX_TEMPORARY_BATCHES, 2);
  assert.equal(EXTENSION_SNAP_TIMEOUT_MS, 500);
  const m = new ConstructionManager();
  m.proposeConstructionItems(batch(A), false, 0);
  m.proposeConstructionItems(batch(B), false, 10);
  assert.equal(m.involvesAllGivenRealItems([A, B]), true);
  m.proposeConstructionItems(batch(C), false, 20);
  assert.equal(m.involvesAllGivenRealItems([C]), false, "full: C is only proposed");
  assert.equal(m.dueAt, 520);
  assert.equal(m.tick(400), false, "not yet");
  assert.equal(m.involvesAllGivenRealItems([C]), false);
  assert.equal(m.tick(520), true);
  assert.equal(m.involvesAllGivenRealItems([C]), true);
  assert.equal(m.involvesAllGivenRealItems([A]), false, "A was the oldest and went");
  assert.equal(m.involvesAllGivenRealItems([B]), true);
  assert.equal(m.dueAt, null);
});

test("a temporary batch of items already shown adds nothing; the same proposal twice is one", () => {
  const m = new ConstructionManager();
  m.proposeConstructionItems(batch(A), false, 0);
  m.proposeConstructionItems(batch(A), false, 5); // the same as the last accepted: ignored
  assert.equal(m.getConstructionItems().length, 1);
  m.proposeConstructionItems(batch(B), false, 10);
  m.proposeConstructionItems(batch(A, B), false, 20); // a different proposal, but every item is shown already
  assert.equal(m.getConstructionItems().length, 2);
  m.tick(600);
  assert.equal(m.getConstructionItems().length, 2, "nothing new was involved: not added");
});

test("a proposal that is cancelled never comes due; a new one replaces a pending one", () => {
  const m = new ConstructionManager();
  m.proposeConstructionItems(batch(A), false, 0);
  m.proposeConstructionItems(batch(B), false, 0);
  m.proposeConstructionItems(batch(C), false, 100);
  assert.equal(m.dueAt, 600);
  m.cancelProposal();
  assert.equal(m.dueAt, null);
  assert.equal(m.tick(10_000), false);
  m.proposeConstructionItems(batch(C), false, 1000);
  m.proposeConstructionItems(batch(D), false, 1100);
  assert.equal(m.dueAt, 1600, "D replaced C and restarted the wait");
  m.tick(1600);
  assert.equal(m.involvesAllGivenRealItems([D]), true);
  assert.equal(m.involvesAllGivenRealItems([C]), false);
});

test("construction geometry that belongs to no item is always involved; clear forgets everything", () => {
  const m = new ConstructionManager();
  assert.equal(m.involvesAllGivenRealItems([null]), true);
  m.proposeConstructionItems(batch(A), true, 0);
  assert.equal(m.drawables().length, 1);
  assert.equal(m.drawables()[0]!.persistent, true);
  m.clear();
  assert.equal(m.hasActiveConstruction(), false);
  assert.equal(m.involvesAllGivenRealItems([A]), false);
});

test("the snap line: directions are made unique and point right; an anchor on a direction ends the line, any other starts a new one", () => {
  const s = new SnapLineManager();
  assert.deepEqual(s.directions, [
    [1, 0],
    [0, 1],
  ]);
  s.setDirections([
    [2, 0],
    [0, -3],
    [1, 1],
    [-1, -1],
    [0, 0],
    [1, -1],
  ]);
  assert.deepEqual(s.directions, [
    [1, 0],
    [0, 1],
    [1, 1],
    [1, -1],
  ]);
  s.setSnappedAnchor([100, 100]); // no origin yet: it starts one
  assert.deepEqual(s.snapLineOrigin, [100, 100]);
  assert.equal(s.snapLineEnd, null);
  s.setSnappedAnchor([300, 100]); // on the horizontal: the end
  assert.deepEqual(s.snapLineEnd, [300, 100]);
  assert.equal(s.activeDirection, 0);
  assert.equal(s.hasCompleteSnapLine(), true);
  s.setSnappedAnchor([400, 500]); // on no direction: a new origin
  assert.deepEqual(s.snapLineOrigin, [400, 500]);
  assert.equal(s.snapLineEnd, null);
  assert.equal(s.activeDirection, null);
  s.setSnappedAnchor([500, 600]); // on the 45-degree diagonal
  assert.equal(s.activeDirection, 2);
  s.setDirections([]);
  assert.equal(s.snapLineOrigin, null, "no directions: no snap line");
});

test("the snap line pulls the cursor onto a direction within range, at the grid point along it", () => {
  const s = new SnapLineManager();
  s.setSnapLineOrigin([1000, 1000]);
  // 100 off the horizontal, far along it; the nearest grid point is (6000, 3000), only its x counts.
  near(s.nearestSnapLinePoint([6020, 1100], [6000, 1000], null, 250, [1000, 1000]), [6000, 1000]);
  // Out of range of both lines.
  assert.equal(s.nearestSnapLinePoint([6020, 1400], [6000, 1000], null, 250, [1000, 1000]), null);
  // Near the vertical.
  near(s.nearestSnapLinePoint([1100, 7020], [1000, 7000], null, 250, [1000, 1000]), [1000, 7000]);
  // An anchor within range of the cursor wins over the line.
  assert.equal(s.nearestSnapLinePoint([6020, 1100], [6000, 1000], 100, 250, [1000, 1000]), null);
  // No grid: the projection.
  near(s.nearestSnapLinePoint([6020, 1100], [6020, 1100], null, 250), [6020, 1000]);
});

test("a line captures the cursor only within the snap range of it", () => {
  const s = new SnapLineManager();
  s.setSnapLineOrigin([0, 0]);
  assert.ok(s.nearestSnapLinePoint([10_000, 240], [10_000, 0], null, 250, [1000, 1000]), "240 off a line 10 mm long: captured");
  assert.equal(s.nearestSnapLinePoint([10_000, 290], [10_000, 0], null, 250, [1000, 1000]), null, "290 off: not");
  // (`GetNearestSnapLinePoint`'s "escape" test -- more than twice the range away and more than 4 degrees off the line -- is written after the range test and so can
  // never fire; the port keeps it in the same place.)
});

test("a diagonal direction snaps to the grid point closest to the line", () => {
  const s = new SnapLineManager();
  s.setDirections([[1, 1]]);
  s.setSnapLineOrigin([0, 0]);
  // The cursor is near the diagonal y = x at (5050, 4980): the grid point (5000, 5000) is on it.
  near(s.nearestSnapLinePoint([5050, 4980], [5000, 5000], null, 250, [1000, 1000]), [5000, 5000]);
});

test("the snap manager adds the snap line's own lines to the construction items, the active one double", () => {
  const m = new SnapManager();
  assert.equal(m.constructionItems().length, 0);
  m.snapLines.setSnapLineOrigin([10, 20]);
  m.snapLines.setSnapLineEnd([510, 20]);
  const batches = m.constructionItems();
  assert.equal(batches.length, 1);
  assert.equal(batches[0]![0]!.source, "snapLine");
  assert.equal(batches[0]![0]!.item, null);
  assert.deepEqual(
    batches[0]![0]!.constructions.map((c) => c.lineWidth),
    [2, 1]
  );
  m.setReferenceOnlyPoints([[1, 2]]);
  assert.equal(m.isReferenceOnly([1, 2]), true);
  assert.equal(m.isReferenceOnly([1, 3]), false);
  m.clear();
  assert.equal(m.snapLines.snapLineOrigin, null);
});
