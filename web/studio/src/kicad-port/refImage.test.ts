import { test } from "node:test";
import assert from "node:assert/strict";
import { base64Of, checkImageScale, DEFAULT_PPI, greyscaleRgba, imageBoxHitDistance, imageBoxNm, imageHeader, imageSizeNm, SCALE_PPM, scaleForHeightNm, scaleForWidthNm, scalePpmOf, scaleText, sniffImage, withPngPpi } from "./refImage";

/** A PNG of `w` by `h` pixels with a `pHYs` chunk of `perMetre` pixels per metre (0: none); the pixel data is not there (a header is all that is read). */
function png(w: number, h: number, perMetre = 0): Uint8Array {
  const be = (n: number) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
  const chunk = (type: string, data: number[]) => [...be(data.length), ...[...type].map((c) => c.charCodeAt(0)), ...data, 0, 0, 0, 0];
  return Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, ...chunk("IHDR", [...be(w), ...be(h), 8000, 6000, 0, 0, 0]), ...(perMetre ? chunk("pHYs", [...be(perMetre), ...be(perMetre), 1]) : []), ...chunk("IEND", [])]);
}

test("the size of an image is its pixels at their definition times the scale", () => {
  // 300 pixels at 300 ppi are an inch, at 96 ppi 3.125 inches
  assert.deepEqual(imageSizeNm([300_000, 150_000], 300, 1_000_000_000), [25_400_000, 12_700_000]);
  assert.deepEqual(imageSizeNm([96_000, 48_000], 96, 1_000_000_000), [25_400_000, 12_700_000]);
  assert.deepEqual(imageSizeNm([300_000, 150_000], 300, 500_000_000), [12_700_000, 6_350_000]);
  assert.deepEqual(imageSizeNm([300_000, 150_000], 300, 2_540_000_000), [64_516_000, 32_258_000]);
});

test("the scale a width or a height asks for", () => {
  assert.equal(scaleForWidthNm([300_000, 150_000], 300, 12_700_000), 500_000_000);
  assert.equal(scaleForHeightNm([300_000, 150_000], 300, 12_700_000), 1_000_000_000);
  assert.equal(scaleForHeightNm([300_000, 150_000], 300, 25_400_000), 2_000_000_000);
  assert.equal(scalePpmOf("0.254"), 254_000_000);
  assert.equal(scaleText(254_000_000), "0.254");
  assert.equal(scaleText(1_000_000_000), "1");
  assert.ok(Number.isNaN(scalePpmOf("abc")));
});

test("PANEL_IMAGE_EDITOR::CheckValues: a scale is positive, leaves 15 pixels and asks above 6000", () => {
  assert.deepEqual(checkImageScale([300_000, 300_000], 1_000_000_000), { ok: true });
  assert.equal(checkImageScale([300_000, 300_000], -1).ok, false);
  const small = checkImageScale([300_000, 300_000], 40_000_000);
  assert.ok(!small.ok && /too small \(1\.02 mm or 40_000\.0 mil\)/.test(small.error), JSON.stringify(small));
  const big = checkImageScale([3_000_000, 3_000_000], 3_000_000_000);
  assert.ok(big.ok && /very large \(762_000\.0 mm or 30_000\.00 in\)\. Are you sure\?/.test(big.confirm ?? ""), JSON.stringify(big));
  // the smaller side decides "too small", the larger one "too large"
  assert.equal(checkImageScale([1_000_000, 10_000], 1_000_000_000).ok, false);
  assert.deepEqual(checkImageScale([6_000_000, 6_000_000], 1_000_000_000), { ok: true });
});

test("the header of a PNG: its size, and its definition from pHYs (pixels per metre) or 300 when it says none", () => {
  assert.deepEqual(imageHeader(png(640, 480)), { format: "png", px: [640, 480], ppi: 300 });
  // 3780 pixels per metre: 37.8 per centimetre, 96 per inch
  assert.deepEqual(imageHeader(png(10, 20, 3780)), { format: "png", px: [10, 20], ppi: 96 });
  // 11811 pixels per metre: 300 per inch
  assert.equal(imageHeader(png(10_000, 20_000, 11_811_000))!.ppi, 300_000);
  assert.equal(sniffImage(png(1, 1)), "png");
  assert.equal(imageHeader(Uint8Array.from([1, 2000, 3000])), null);
});

test("the header of a JPEG: the start of frame gives the size, JFIF the definition", () => {
  const jpeg = Uint8Array.from([
    0xff, 0xd8,
    // JFIF: length 16, version 1.1, units 1 (dpi), 72 by 72, no thumbnail
    0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x01, 0x00, 0x48, 0x00, 0x48, 0x00, 0x00,
    // SOF0: length 17, 8 bits, height 100, width 200, 3 components
    0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x64, 0x00, 0xc8, 0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01,
  ]);
  assert.deepEqual(imageHeader(jpeg), { format: "jpeg", px: [200, 100], ppi: 72 });
});

test("base64 of bytes, as wxBase64Encode makes it", () => {
  assert.equal(base64Of(new TextEncoder().encode("Man")), "TWFu");
  assert.equal(base64Of(new TextEncoder().encode("Ma")), "TWE=");
  assert.equal(base64Of(new TextEncoder().encode("M")), "TQ==");
  assert.equal(base64Of(new Uint8Array(0)), "");
});

test("an image's box is centred on its position, and a click is inside it or its distance away", () => {
  // 60 x 40 px at 254 ppi are 6 x 4 mm; at scale 2 they are 12 x 8
  assert.deepEqual(imageBoxNm([10_000_000, 20_000_000], [60_000, 40_000], 254, SCALE_PPM), [7_000_000, 18_000_000, 13_000_000, 22_000_000]);
  assert.deepEqual(imageBoxNm([10_000_000, 20_000_000], [60_000, 40_000], 254, 2 * SCALE_PPM), [4_000_000, 16_000_000, 16_000_000, 24_000_000]);
  const box = imageBoxNm([0, 0], [60_000, 40_000], 254, SCALE_PPM);
  assert.equal(imageBoxHitDistance(box, 0, 0), 0);
  assert.equal(imageBoxHitDistance(box, 3_000_000, 2_000_000), 0, "on the edge is a hit");
  assert.equal(imageBoxHitDistance(box, 3_500_000, 2_000_000), 500_000);
  assert.equal(imageBoxHitDistance(box, 6_000_000, 6_000_000), Math.hypot(3_000_000, 4_000_000));
});

test("greyscale is the weighted sum wxImage uses, rounded, and leaves the alpha alone", () => {
  const px = new Uint8ClampedArray([255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 10, 20, 30, 255]);
  greyscaleRgba(px);
  assert.deepEqual(Array.from(px), [76, 76, 76, 255, 150, 150, 150, 128, 29, 29, 29, 0, 18, 18, 18, 255]);
});

test("a PNG gets the definition it is to be read at, in place of the one it has", () => {
  // IHDR (13 bytes of data) for a 3 x 2 image, then IDAT and IEND; no pHYs
  const be = (n: number): number[] => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
  const chunk = (type: string, data: number[]): number[] => [...be(data.length), ...Array.from(type, (c) => c.charCodeAt(0)), ...data, 0, 0, 0, 0];
  const png = Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, ...chunk("IHDR", [...be(3000), ...be(2000), 8000, 2000, 0, 0, 0]), ...chunk("IDAT", [1, 2000, 3000]), ...chunk("IEND", [])]);
  assert.equal(imageHeader(png)!.ppi, DEFAULT_PPI);
  const out = withPngPpi(png, 254);
  assert.deepEqual(imageHeader(out), { format: "png", px: [3, 2], ppi: 254 });
  // again, with another: the chunk is replaced, not added
  const again = withPngPpi(out, 96);
  assert.equal(imageHeader(again)!.ppi, 96_000);
  assert.equal(again.length, out.length);
  // a file that is not a PNG is left alone
  const jpeg = Uint8Array.from([0xff, 0xd8, 0xff, 0xd9]);
  assert.equal(withPngPpi(jpeg, 254), jpeg);
});
