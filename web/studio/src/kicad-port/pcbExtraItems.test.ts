import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardRefImage } from "../api/pcbItemTypes";
import type { BoardState } from "../api/types";
import { pcbItemBoxes } from "./itemBoxes";
import { movableItem } from "./pcbEditActions";
import { splitCarried } from "./pcbCarry";
import { extraDeleteCmd, extraHitDistance, extraItemById, extraItemIds, extraItemLabel, extraItems, pointBounds, refImageBounds } from "./pcbExtraItems";
import { boardTextBoxOf } from "./pcbTextBox";

const image: BoardRefImage = { id: "img_a", layer: "Cmts.User", x: 50_000, y: 40_000, w: 12_000, h: 8_000, scale_ppm: 1_000_000, px: [120, 80], ppi: 254, hash: "h" };
const box = boardTextBoxOf({ id: "tbx_a", layer: "F.SilkS", text: "hello", start: { x: 0, y: 0 }, end: { x: 10_000, y: 5_000 }, margin_left_um: 500, margin_top_um: 500, margin_right_um: 500, margin_bottom_um: 500, stroke_width_um: 100, size_um: 1_000, thickness_um: 150, halign: "left", valign: "top" });

const board = (): BoardState =>
  ({
    name: "t",
    dir: "/t",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100,
    parts: [],
    rules: [],
    routing: null,
    drawings: { shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {}, text_boxes: [box], images: [image] },
    checks: [],
    activity: [],
    job: "idle",
    locked: [],
  }) as unknown as BoardState;

test("the family lists text boxes and images, by id, with the box each fills", () => {
  const b = board();
  assert.deepEqual(extraItems(b).map((i) => [i.kind, i.id]), [["textbox", "tbx_a"], ["image", "img_a"]]);
  assert.deepEqual(extraItemIds(b), ["tbx_a", "img_a"]);
  assert.equal(extraItemById(b, "img_a")?.kind, "image");
  assert.equal(extraItemById(b, "nope"), undefined);
  // the image's box is centred on its position
  assert.deepEqual(refImageBounds(image), [44_000, 36_000, 56_000, 44_000]);
  assert.deepEqual(extraItemById(b, "img_a")?.bounds, [44_000, 36_000, 56_000, 44_000]);
  assert.deepEqual(extraItemById(b, "img_a")?.position, [50_000, 40_000]);
});

test("an image is hit anywhere in its box, deleted by its own verb and named as KiCad names it", () => {
  const b = board();
  const item = extraItemById(b, "img_a")!;
  assert.equal(extraHitDistance(item, 50_000, 40_000), 0);
  assert.equal(extraHitDistance(item, 56_000, 44_000), 0);
  assert.equal(extraHitDistance(item, 56_300, 40_000), 300);
  assert.deepEqual(extraDeleteCmd(item), { op: "pcb_item", verb: "delete_ref_image", id: "img_a" });
  assert.deepEqual(extraDeleteCmd(extraItemById(b, "tbx_a")!), { op: "pcb_item", verb: "delete_text_box", id: "tbx_a" });
  assert.equal(extraItemLabel(b, "img_a"), "Reference Image [Cmts.User]");
});

test("Zoom to Selection, the carry and the move anchor find the family too", () => {
  const b = board();
  const boxes = pcbItemBoxes(b);
  assert.deepEqual(boxes.get("img_a"), [44_000, 36_000, 56_000, 44_000]);
  assert.ok(boxes.has("tbx_a"));
  const carried = splitCarried(b, ["img_a"]);
  assert.deepEqual(carried.moving.drawings?.images?.map((i) => i.id), ["img_a"]);
  assert.deepEqual(carried.still.drawings?.images, []);
  assert.deepEqual(carried.still.drawings?.text_boxes?.map((i) => i.id), ["tbx_a"]);
  assert.deepEqual(movableItem(b as never, "img_a"), { kind: "image", at: [50_000, 40_000] });
});

test("a point is a mark: its box is centred, a click hits the bars of its X or the disc of its ring, and it is deleted by its own verb", () => {
  const point = { id: "pnt_a", layer: "F.SilkS", x: 10_000, y: 20_000, size: 2_000 };
  const b = board();
  b.drawings!.points = [point];
  const item = extraItemById(b, "pnt_a")!;
  assert.equal(item.kind, "point");
  assert.deepEqual(item.bounds, [9_000, 19_000, 11_000, 21_000]);
  assert.deepEqual(pointBounds(point), [9_000, 19_000, 11_000, 21_000]);
  assert.equal(extraHitDistance(item, 10_000, 20_000), 0, "the middle is inside the ring's disc");
  assert.equal(extraHitDistance(item, 10_500, 20_000), 0, "inside the ring (radius a quarter of the size: 500)");
  assert.equal(extraHitDistance(item, 10_900, 20_900), 0, "on a bar of the X, near its end");
  assert.ok(Math.abs(extraHitDistance(item, 11_000, 20_000) - Math.min(500, 707.1067)) < 1e-6, "off the bars and the ring: the nearer of the two");
  assert.ok(extraHitDistance(item, 12_000, 20_000) > 700, "well away");
  assert.deepEqual(extraDeleteCmd(item), { op: "pcb_item", verb: "delete_point", id: "pnt_a" });
  assert.equal(extraItemLabel(b, "pnt_a"), "Point [F.SilkS]");
  assert.deepEqual(extraItemIds(b), ["tbx_a", "img_a", "pnt_a"]);
  assert.deepEqual(movableItem(b as never, "pnt_a"), { kind: "point", at: [10_000, 20_000] });
  assert.deepEqual(splitCarried(b, ["pnt_a"]).moving.drawings?.points?.map((p) => p.id), ["pnt_a"]);
  assert.deepEqual(pcbItemBoxes(b).get("pnt_a"), [9_000, 19_000, 11_000, 21_000]);
});
