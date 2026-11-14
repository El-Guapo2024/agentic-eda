import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardRefImage } from "../api/pcbItemTypes";
import { imageEditPoints, imagePointAt, imagePointCmds, MIN_DRAG_SIDE_NM, moveImagePoint, scaleImageBy } from "./refImagePointEdit";

// 120 x 80 px at 254 ppi: 12 x 8 mm at scale 1, centred at (50, 40) mm
const img = (offset?: { x: number; y: number }): BoardRefImage => ({ id: "img_a", layer: "Cmts.User", x: 50_000_000, y: 40_000_000, w: 12_000_000, h: 8_000_000, scale_ppm: 1_000_000, px: [120, 80], ppi: 254, hash: "h", ...(offset ? { offset } : {}) });

test("MakePoints: the four corners, then the transform origin (the centre plus the offset)", () => {
  assert.deepEqual(
    imageEditPoints(img({ x: 1_000_000, y: -2_000_000 })).map((p) => [p.id, p.pos]),
    [
      ["tl", [44_000_000, 36_000_000]],
      ["tr", [56_000_000, 36_000_000]],
      ["br", [56_000_000, 44_000_000]],
      ["bl", [44_000_000, 44_000_000]],
      ["origin", [51_000_000, 38_000_000]],
    ]
  );
  assert.equal(imagePointAt(img(), [56_000_100, 43_999_900], 1_000), "br");
  assert.equal(imagePointAt(img(), [50_000_000, 40_000_000], 1_000), "origin");
  assert.equal(imagePointAt(img(), [52_000_000, 40_000_000], 1_000), null);
});

test("the origin point moves the transform origin only", () => {
  const moved = moveImagePoint(img(), "origin", [53_000_000, 41_000_000]);
  assert.deepEqual([moved.x, moved.y, moved.scale_ppm, moved.offset], [50_000_000, 40_000_000, 1_000_000, { x: 3_000_000, y: 1_000_000 }]);
  assert.deepEqual(imagePointCmds(img(), moved), [{ op: "pcb_item", verb: "set_ref_image_offset", id: "img_a", offset: { x: 3_000_000, y: 1_000_000 } }]);
});

test("a corner scales the image about its origin; the origin stays put", () => {
  // the bottom right corner dragged twice as far from the centre: twice the size, the centre kept
  const twice = moveImagePoint(img(), "br", [62_000_000, 48_000_000]);
  assert.deepEqual([twice.x, twice.y, twice.w, twice.h, twice.scale_ppm], [50_000_000, 40_000_000, 24_000_000, 16_000_000, 2_000_000]);
  // with the origin on the top left corner: dragging the bottom right one halfway keeps the top left corner where it is
  const o = img({ x: -6_000_000, y: -4_000_000 });
  const half = moveImagePoint(o, "br", [50_000_000, 40_000_000]);
  assert.deepEqual([half.x - half.w / 2, half.y - half.h / 2, half.scale_ppm, half.offset], [44_000_000, 36_000_000, 500_000, { x: -3_000_000, y: -2_000_000 }]);
  assert.deepEqual(imagePointCmds(o, half), [
    { op: "pcb_item", verb: "edit_ref_image", id: "img_a", layer: "Cmts.User", at: { x: 47_000_000, y: 38_000_000 }, scale_ppm: 500_000 },
    { op: "pcb_item", verb: "set_ref_image_offset", id: "img_a", offset: { x: -3_000_000, y: -2_000_000 } },
  ]);
});

test("a corner dragged across the origin, or nearly onto it, stops at 50 mils", () => {
  const crossed = moveImagePoint(img(), "tl", [60_000_000, 50_000_000]);
  // each side clamped to 50 mils and the smaller of the two ratios taken (`std::min( newWidth / oldSize.x, newHeight / oldSize.y )`): the wider side gets 50 mils
  assert.equal(crossed.scale_ppm, Math.round(1_000_000 * (MIN_DRAG_SIDE_NM / 12_000_000)));
  assert.ok(Math.abs(crossed.w - MIN_DRAG_SIDE_NM) <= 1_000, `${crossed.w} x ${crossed.h}`);
  assert.deepEqual(scaleImageBy(img(), 0), img(), "a ratio that is not positive changes nothing");
});
