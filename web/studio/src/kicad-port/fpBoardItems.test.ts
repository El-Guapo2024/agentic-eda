import { test } from "node:test";
import assert from "node:assert/strict";
import type { Part } from "../api/types";
import type { BoardBarcode, BoardPoint, BoardRefImage, BoardTextBox } from "../api/pcbItemTypes";
import { fpDimensionHitDistance, fpItemsBounds, fpItemsHitDistance, fpTableHitDistance, onPrivateLayer, parseFpItemId, shownFootprintImages } from "./fpBoardItems";
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
  assert.deepEqual(parseFpItemId("U1~dim_000000000005"), { ref: "U1", item: "dim_000000000005" }, "a dimension (UI-LEFT PP14)");
  assert.deepEqual(parseFpItemId("U1~tbl_000000000006"), { ref: "U1", item: "tbl_000000000006" }, "a table");
  assert.equal(parseFpItemId("U1~t0"), null, "a footprint's text is not one");
  assert.equal(parseFpItemId("img_000000000002"), null);
});

test("a click on any of a footprint's items is a click on the footprint, while its layer and side show", () => {
  assert.equal(fpItemsHitDistance(part, 12_000_000, 11_000_000, {}), 0, "inside the text box");
  assert.equal(fpItemsHitDistance(part, 5_000_000, 0, {}), 0, "on the point");
  // a reference image is not drawn on the board ("Never draw footprint reference images on board", UI-LEFT-3 P3-31): the nearest is the barcode's box, 4 mm away
  assert.equal(fpItemsHitDistance(part, 0, 0, {}), 4_000_000, "not on the image");
  assert.equal(fpItemsHitDistance(part, 12_000_000, 11_000_000, { "obj:footprints_front": false }), Infinity, "a hidden side hides the footprint's items");
});

test("the footprint's box counts its text boxes and barcodes, not its images; without text the annotations go", () => {
  assert.deepEqual(fpItemsBounds(part), [-6_000_000, -1_000_000, 14_050_000, 12_050_000]);
  const withText = footprintBox(part, true);
  const without = footprintBox(part, false);
  assert.deepEqual(withText, [-6_000_000, -1_000_000, 14_050_000, 12_050_000]);
  assert.deepEqual(without, [-250_000, -250_000, 14_050_000, 12_050_000], "the barcode on Dwgs.User is an annotation");
});

test("the board draws no footprint's reference image (`PCB_PAINTER::Draw`, UI-LEFT-3 P3-31)", () => {
  assert.equal(shownFootprintImages({ parts: [part] }, {}).length, 0);
});

test("an item on one of the footprint's private layers is not drawn, boxed or hit on the board (`GetPrivateLayers`, UI-LEFT-3 P3-31)", () => {
  const privateDwgs = { ...part, private_layers: ["Dwgs.User"] } as unknown as Part;
  // the barcode is on Dwgs.User: it no longer counts in the box, nor is it hit
  assert.deepEqual(fpItemsBounds(privateDwgs), fpItemsBounds({ ...part, items: { ...part.items, barcodes: [] } } as unknown as Part));
  assert.notEqual(fpItemsHitDistance(privateDwgs, 0, 0, {}), 4_000_000);
  assert.equal(onPrivateLayer(privateDwgs, "Dwgs.User"), true);
  assert.equal(onPrivateLayer(privateDwgs, "F.SilkS"), false);
});

// UI-LEFT PP14: a footprint's dimension and table are hit by their lines and cells (a click picks the footprint), and count in its box -- the dimension only with the text
test("a footprint's dimension and table are hit and boxed with it", () => {
  const dim = { id: "U1~dim_000000000005", layer: "F.Fab", lines: [[[0, 20_000_000], [10_000_000, 20_000_000]]], text_at: [5_000_000, 19_000_000] };
  const cell = { id: "c", col_span: 1, row_span: 1, corners: [[20_000_000, 0], [24_000_000, 0], [24_000_000, 2_000_000], [20_000_000, 2_000_000]] };
  const table = { id: "U1~tbl_000000000006", layer: "F.Fab", cells: [cell], bbox: [20_000_000, 0, 24_000_000, 2_000_000] };
  assert.equal(fpDimensionHitDistance(dim as never, 5_000_000, 21_000_000), 1_000_000, "a millimetre under the crossbar");
  assert.equal(fpTableHitDistance(table as never, 22_000_000, 1_000_000), 0, "inside the cell");
  const withBoth = { ...part, items: { ...part.items, dimensions: [dim], tables: [table] } } as unknown as Part;
  assert.equal(fpItemsHitDistance(withBoth, 22_000_000, 1_000_000, {}), 0);
  const without = { ...withBoth, items: { ...part.items } } as unknown as Part;
  assert.equal(fpItemsHitDistance(withBoth, 5_000_000, 20_000_000, { "F.Fab": false }), fpItemsHitDistance(without, 5_000_000, 20_000_000, {}), "a hidden layer hides them");
  assert.deepEqual(fpItemsBounds(withBoth), [-6_000_000, -1_000_000, 24_000_000, 20_000_000]);
  assert.deepEqual(footprintBox(withBoth, false)?.[3], 12_050_000, "without the text the dimension goes");
  assert.deepEqual(footprintBox(withBoth, false)?.[2], 24_000_000, "the table stays");
});
