import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardBarcode } from "../api/pcbItemTypes";
import { barcodeBounds, barcodeHitDistance, barcodeLabel, barcodeProblem, barcodeShape, barcodeShapeKey, barcodeToCmd, clampEcc, eccChoices, hasErrorCorrection, hitRects, knockoutMinMarginNm, newBarcode, setBarcodeShape, symbolCorners, toLocal } from "./barcodeShapes";

const code = (over: Partial<BoardBarcode> = {}): BoardBarcode => ({
  id: "bar_a",
  layer: "F.SilkS",
  x: 20_000,
  y: 30_000,
  angle: 0,
  w: 10_000,
  h: 6_000,
  text: "HELLO",
  text_height: 1_500,
  kind: "qr",
  ecc: "L",
  show_text: true,
  knockout: false,
  margin_x: 0,
  margin_y: 0,
  ...over,
});

const near = (a: number, b: number, tol = 1) => assert.ok(Math.abs(a - b) <= tol, `${a} is not ${b}`);

test("the cache key has what the shape depends on, the id first, and not the place", () => {
  const a = code();
  assert.equal(barcodeShapeKey(a), barcodeShapeKey(code({ x: 1, y: 2 })));
  for (const change of [{ w: 11_000 }, { h: 7_000 }, { angle: 90_000 }, { layer: "B.SilkS" }, { show_text: false }, { knockout: true }, { margin_x: 1 }, { kind: "datamatrix" as const }, { ecc: "H" as const }, { text: "X" }, { text_height: 1_000 }, { id: "bar_b" }]) {
    assert.notEqual(barcodeShapeKey(a), barcodeShapeKey(code(change)), JSON.stringify(change));
  }
  // what the text stands for is what is encoded
  assert.notEqual(barcodeShapeKey(code({ text: "${REV}", shown: "A" })), barcodeShapeKey(code({ text: "${REV}", shown: "B" })));
});

test("the symbol's corners are turned about the centre the way KiCad turns (counter-clockwise on the screen, y down)", () => {
  assert.deepEqual(symbolCorners(code()), [[15_000, 27_000], [25_000, 27_000], [25_000, 33_000], [15_000, 33_000]]);
  // a quarter turn counter-clockwise on the screen: the top right corner goes to the top left
  const c = symbolCorners(code({ angle: 90_000 }));
  near(c[1]![0], 17_000);
  near(c[1]![1], 25_000);
  // a point of the board in the barcode's own frame: the way back
  const [lx, ly] = toLocal(code({ angle: 90_000 }), 17_000, 25_000);
  near(lx, 5_000);
  near(ly, -3_000);
});

test("the bounding box is the symbol's turned box until the shape is known, and everything the plot has afterwards", () => {
  const b = code({ id: "bar_bounds" });
  assert.deepEqual(barcodeBounds(b), [15_000, 27_000, 25_000, 33_000]);
  const turned = barcodeBounds(code({ id: "bar_bounds", angle: 90_000 }));
  near(turned[0], 17_000);
  near(turned[2], 23_000);
  near(turned[1], 25_000);
  near(turned[3], 35_000);
  setBarcodeShape(barcodeShapeKey(b), { status: "ready", paths: [[[[-5_000, -3_000], [5_000, -3_000], [5_000, 3_000], [-5_000, 3_000]]], [[[-1_000, 4_000], [1_000, 4_000], [1_000, 5_500], [-1_000, 5_500]]]] });
  assert.deepEqual(barcodeBounds(b), [15_000, 27_000, 25_000, 35_500]);
  assert.equal(barcodeShape(barcodeShapeKey(b))?.status, "ready");
});

test("a click hits the symbol's box and the text's, turned with the barcode, and nothing between or beyond", () => {
  const b = code({ id: "bar_hit" });
  setBarcodeShape(barcodeShapeKey(b), { status: "ready", paths: [[[[-5_000, -3_000], [5_000, -3_000], [5_000, 3_000], [-5_000, 3_000]]], [[[-1_000, 4_000], [1_000, 4_000], [1_000, 5_500], [-1_000, 5_500]]]] });
  assert.equal(hitRects(b).length, 2, "the symbol and the text");
  assert.equal(barcodeHitDistance(b, 20_000, 30_000), 0, "on the symbol");
  assert.equal(barcodeHitDistance(b, 20_000, 35_000), 0, "on the text (5 mm under the centre, 2 mm under the symbol)");
  assert.equal(barcodeHitDistance(b, 24_000, 35_000), 2_000, "under the symbol but beside the text: 2 mm from the symbol, 3 mm from the text");
  assert.ok(barcodeHitDistance(b, 40_000, 30_000) > 10_000);
  // text hidden: only the symbol
  const hidden = code({ id: "bar_hit", show_text: false });
  assert.equal(hitRects(hidden).length, 1);
  // turned: the box turns, the text goes beside the symbol
  const turned = code({ id: "bar_hit_turned", angle: 90_000 });
  setBarcodeShape(barcodeShapeKey(turned), { status: "ready", paths: [[[[-3_000, -5_000], [3_000, -5_000], [3_000, 5_000], [-3_000, 5_000]]], [[[4_000, -1_000], [5_500, -1_000], [5_500, 1_000], [4_000, 1_000]]]] });
  // the polygons are already turned: the symbol (6 x 10 turned) and the text 4 mm to the right of the centre; turned back, the text is under the symbol (y from 4 mm)
  assert.equal(hitRects(turned).length, 2);
  assert.equal(barcodeHitDistance(turned, 25_000, 30_000), 0, "on the text, to the right of the turned symbol");
});

test("the kinds, their error correction and the levels Micro QR has", () => {
  assert.deepEqual(hasErrorCorrection("qr"), true);
  assert.deepEqual(hasErrorCorrection("microqr"), true);
  assert.deepEqual(["code39", "code128", "datamatrix"].map((k) => hasErrorCorrection(k as never)), [false, false, false]);
  assert.deepEqual(eccChoices("qr").map((c) => c.value), ["L", "M", "Q", "H"]);
  assert.deepEqual(eccChoices("microqr").map((c) => c.value), ["L", "M", "Q"]);
  assert.equal(clampEcc("microqr", "H"), "Q", "SetErrorCorrection: Micro QR has no High");
  assert.equal(clampEcc("qr", "H"), "H");
  assert.equal(clampEcc("microqr", "M"), "M");
});

test("a new barcode is the QR code of PCB_BARCODE( parent ), 40 mm, level L, text shown; the verbs take it back from the state's record", () => {
  assert.deepEqual(newBarcode("F.SilkS", { x: 5, y: 6 }, 1_000), { layer: "F.SilkS", at: { x: 5, y: 6 }, angle: 0, width_nm: 40_000, height_nm: 40_000, text: "", text_height_nm: 1_000, kind: "qr", ecc: "L", show_text: true, knockout: false, margin_x_nm: 0, margin_y_nm: 0 });
  const cmd = barcodeToCmd(code({ angle: 90_000, knockout: true, margin_x: 700, margin_y: 800, ecc: "Q" }));
  assert.deepEqual(cmd, { layer: "F.SilkS", at: { x: 20_000, y: 30_000 }, angle: 90_000, width_nm: 10_000, height_nm: 6_000, text: "HELLO", text_height_nm: 1_500, kind: "qr", ecc: "Q", show_text: true, knockout: true, margin_x_nm: 700, margin_y_nm: 800 });
  // a knockout is at least a tenth of the smaller side, rounded up to 0.1 mm (`AssembleBarcode`)
  assert.equal(knockoutMinMarginNm(10_000, 6_000), 600);
  assert.equal(knockoutMinMarginNm(40_000, 40_000), 4_000);
  assert.equal(knockoutMinMarginNm(1_050, 5_000), 200);
});

test("KiCad's words for a barcode in a menu and for a text a kind cannot take", () => {
  assert.equal(barcodeLabel(code()), "Barcode 'HELLO' on F.SilkS");
  assert.equal(barcodeLabel(code({ text: "${REV}", shown: "B" })), "Barcode 'B' on F.SilkS");
  assert.equal(barcodeLabel(code({ text: "A very long text that goes past twenty-four characters" })), "Barcode 'A very long text that g...' on F.SilkS");
  assert.match(barcodeProblem("code39", "héllo"), /international characters/);
  assert.match(barcodeProblem("qr", "héllo"), /cannot be encoded/);
  assert.match(barcodeProblem("code39", "lower"), /cannot be encoded/);
});
