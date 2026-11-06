import { test } from "node:test";
import assert from "node:assert/strict";
import type { Part } from "../api/types";
import type { BoardBarcode, BoardPoint, BoardRefImage, BoardTextBox } from "../api/pcbItemTypes";
import { fpItemsBounds, fpItemsHitDistance, parseFpItemId, shownFootprintImages } from "./fpBoardItems";
import { footprintBox } from "./footprintBox";

const box: BoardTextBox = {
  id: "U1~tbx_000000000001", layer: "F.SilkS", text: "x", start: [10_000_000, 10_000_000], end: [14_000_000, 12_000_000], pts: [], angle: 0, margins: [0, 0, 0, 0], border: true, stroke_width: 100_000,
  line_style: "solid", stroke_color: null, knockout: false, size: 1_000_000, width: 1_000_000, thickness: 150_000, thickness_auto: false, halign: "left", valign: "top", mirror: false, bold: false, italic: false,
} as unknown as BoardTextBox;
const image: BoardRefImage = { id: "U1~img_000000000002", layer: "F.SilkS", x: 0, y: 0, w: 2_000_000, h: 2_000_000, scale_ppm: 1_000_000, px: [24, 24], ppi: 300, hash: "h" };
const point: BoardPoint = { id: "U1~pnt_000000000003", layer: "F.Fab", x: 5_000_000, y: 0, size: 1_000_000 };
const barcode = { id: "U1~bar_000000000004", layer: "Dwgs.User", x: -5_000_000, y: 0, angle: 0, w: 2_000_000, h: 2_000_000, text: "x", text_height: 1_000_000, kind: "qr", ecc: "L", show_text: false, knockout: false, margin_x: 0, margin_y: 0 } as BoardBarcode;
const part = { ref: "U1", placed: true, at: [0, 0], side: "top", items: { text_boxes: [box], images: [image], points: [point], barcodes: [barcode] } } as unknown as Part;

test("the id of a footprint's item names the footprint and the item", () => {
  assert.deepEqual(parseFpItemId("U1~img_000000000002"), { ref: "U1", item: "img_000000000002" });
  assert.equal(parseFpItemId("U1~t0"), null, "a footprint's text is not one");
  assert.equal(parseFpItemId("img_000000000002"), null);
});

test("a click on any of a footprint's items is a click on the footprint, while its layer and side show", () => {
  assert.equal(fpItemsHitDistance(part, 12_000_000, 11_000_000, {}), 0, "inside the text box");
  assert.equal(fpItemsHitDistance(part, 5_000_000, 0, {}), 0, "on the point");
  assert.equal(fpItemsHitDistance(part, 0, 0, {}), 0, "on the image");
  assert.equal(fpItemsHitDistance(part, 0, 0, { "obj:bitmaps": false }), 4_000_000, "the Images object off: the nearest is then the barcode's box, 4 mm away");
  assert.equal(fpItemsHitDistance(part, 12_000_000, 11_000_000, { "obj:footprints_front": false }), Infinity, "a hidden side hides the footprint's items");
});

test("the footprint's box counts its text boxes and barcodes, not its images; without text the annotations go", () => {
  assert.deepEqual(fpItemsBounds(part), [-6_000_000, -1_000_000, 14_050_000, 12_050_000]);
  const withText = footprintBox(part, true);
  const without = footprintBox(part, false);
  assert.deepEqual(withText, [-6_000_000, -1_000_000, 14_050_000, 12_050_000]);
  assert.deepEqual(without, [-250_000, -250_000, 14_050_000, 12_050_000], "the barcode on Dwgs.User is an annotation");
});

test("the images of the footprints whose side and layer show", () => {
  assert.equal(shownFootprintImages({ parts: [part] }, {}).length, 1);
  assert.equal(shownFootprintImages({ parts: [part] }, { "obj:footprints_front": false }).length, 0);
  assert.equal(shownFootprintImages({ parts: [{ ...part, placed: false }] }, {}).length, 0);
});
