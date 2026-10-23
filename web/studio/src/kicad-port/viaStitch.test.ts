import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardViaStitch } from "../api/pcbItemTypes";
import { bakedPoissonTile, cellForPosition, excludedAt, hasExclusions, Mt19937, stitchesExcluding, stitchHitDistance, stitchPreview, stitchTicks, viaStitchCmd, withLayoutAndMode, withoutExclusions } from "./viaStitch";

const square = (): BoardViaStitch => ({
  id: "vst_a",
  outline: [
    { x: 0, y: 0 },
    { x: 10_000_000, y: 0 },
    { x: 10_000_000, y: 10_000_000 },
    { x: 0, y: 10_000_000 },
  ],
  pitch_nm: 1_000_000,
  net: "GND",
  via: { size_nm: 600_000, drill_nm: 300_000, from_layer: "F.Cu", to_layer: "B.Cu" },
  member_ids: ["via_1"],
});

test("the twister is MT19937 and the tile the server bakes", () => {
  const mt = new Mt19937(5489);
  let last = 0;
  for (let i = 0; i < 10_000; i++) last = mt.next();
  assert.equal(last, 4123659995);
  const tile = bakedPoissonTile();
  // crates/drc/src/via_stitch.rs bakes the same: 42 samples from (0.404105.., 0.559086..).
  assert.equal(tile.length, 42);
  assert.deepEqual(tile[0], [0.404105210211128, 0.5590860228985548]);
});

test("a via's cell is on the stitching's grid, the odd rows of a staggered one half a pitch over (cellForPosition)", () => {
  const v = square();
  assert.deepEqual(cellForPosition(v, [3_000_000, 2_000_000]), [3, 2]);
  assert.deepEqual(cellForPosition({ ...v, origin_offset: { x: 250_000, y: 0 } }, [3_250_000, 2_000_000]), [3, 2]);
  assert.deepEqual(cellForPosition({ ...v, layout: "staggered" }, [3_500_000, 1_000_000]), [3, 1]);
  assert.deepEqual(cellForPosition({ ...v, layout: "poisson" }, [3_123_000, 1_456_000]), [3_123_000, 1_456_000], "no grid: the position");
});

test("Exclude From Stitching keeps the cell, Clear takes the exclusions away, a change of layout or mode drops them", () => {
  const v = excludedAt(square(), [3_000_000, 2_000_000]);
  assert.deepEqual(v.excluded_cells, [[3, 2]]);
  assert.equal(excludedAt(v, [3_000_000, 2_000_000]), v, "already out");
  assert.equal(hasExclusions(v), true);
  assert.equal(hasExclusions(withoutExclusions(v)), false);
  assert.equal(hasExclusions(withLayoutAndMode(v, "staggered", "stitch")), false);
  assert.equal(hasExclusions(withLayoutAndMode(v, "plain", "stitch")), true, "no change keeps them");
  const p = excludedAt({ ...square(), layout: "poisson" }, [3_123_000, 1_456_000]);
  assert.deepEqual(p.excluded_positions, [{ x: 3_123_000, y: 1_456_000 }]);
  // The selected vias of the stitchings they belong to.
  const out = stitchesExcluding([square()], [{ id: "via_1", x: 1_000_000, y: 1_000_000 }, { id: "via_x", x: 0, y: 0 }]);
  assert.deepEqual(out.map((s) => s.excluded_cells), [[[1, 1]]]);
});

test("the outline's ticks point in, every 2 mm (ViewDraw), and the outline is hit inside", () => {
  const ticks = stitchTicks(square().outline);
  assert.equal(ticks.length, 4 * 4, "2, 4, 6, 8 mm along each 10 mm edge");
  const [a, b] = ticks[0]!;
  assert.deepEqual([a, b], [[2_000_000, 0], [2_000_000, 500_000]], "down into the square from the top edge");
  assert.equal(stitchHitDistance(square(), 5_000_000, 5_000_000), 0);
  assert.equal(stitchHitDistance(square(), 12_000_000, 5_000_000), 2_000_000);
});

test("the dialog's sample: six by six at the pitch, a staggered one's odd rows over, a Poisson one two tiles (buildStitchPreview)", () => {
  assert.equal(stitchPreview(1_000, "plain", 0).samples.length, 36);
  assert.deepEqual(stitchPreview(1_000_000, "staggered", 0).samples[6], [500_000, 1_000_000]);
  const p = stitchPreview(1_000_000, "poisson", 0);
  assert.equal(p.extent, 16_000_000);
  assert.ok(p.samples.length > 100);
});

test("the command gives the settings and lets the server place the vias", () => {
  const c = viaStitchCmd("vst_a", square());
  assert.equal(c.op, "pcb_item");
  assert.ok(c.op === "pcb_item" && c.verb === "set_via_stitch" && c.id === "vst_a" && !("vias" in c) && !("id" in c.stitch));
  const n = viaStitchCmd(null, square());
  assert.ok(n.op === "pcb_item" && n.verb === "set_via_stitch" && n.id === undefined);
});
