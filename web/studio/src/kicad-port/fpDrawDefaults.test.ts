import { test } from "node:test";
import assert from "node:assert/strict";
import { fpPolygonFilled, fpShape, fpStrokeWidthNm } from "./fpDrawDefaults";

test("a drawn shape takes its layer class's width (the Footprint Editor's defaults)", () => {
  const widths = ["F.CrtYd", "B.CrtYd", "F.SilkS", "F.Fab", "F.Cu", "B.Cu", "Edge.Cuts", "Dwgs.User", "User.1"].map((l) => fpStrokeWidthNm(l));
  assert.deepEqual(widths, [50_000, 50_000, 100_000, 100_000, 200_000, 200_000, 50_000, 100_000, 100_000]);
});

test("a polygon is unfilled on Edge.Cuts and the courtyards, filled elsewhere (zone_create_helper.cpp)", () => {
  assert.deepEqual(["Edge.Cuts", "F.CrtYd", "B.CrtYd"].map(fpPolygonFilled), [false, false, false]);
  assert.deepEqual(["F.SilkS", "F.Cu", "F.Fab", "Dwgs.User"].map(fpPolygonFilled), [true, true, true, true]);
});

test("the shape the tools commit carries the layer's width and fill", () => {
  const rect = fpShape("rect", [[0, 0], [1_000_000, 1_000_000]], "F.CrtYd");
  assert.deepEqual(rect, { kind: "rect", layer: "F.CrtYd", stroke_width: 50_000, filled: false, start: { x: 0, y: 0 }, end: { x: 1_000_000, y: 1_000_000 } });
  const poly = fpShape("polygon", [[0, 0], [1, 0], [1, 1]], "F.CrtYd");
  assert.equal(poly?.filled, false);
  assert.equal(poly?.stroke_width, 50_000);
  assert.equal(fpShape("polygon", [[0, 0], [1, 0], [1, 1]], "F.SilkS")?.filled, true);
  assert.equal(fpShape("segment", [[0, 0], [1, 0]], "F.Cu")?.stroke_width, 200_000);
  assert.equal(fpShape("circle", [[0, 0]], "F.Cu"), null, "not enough points");
});

test("Preferences > Footprint Editor > Graphics Defaults set the widths the drawing tools start from (UI-LEFT F8)", () => {
  const defaults = { silk: { line_width_nm: 150_000, text_width_nm: 1_000_000, text_height_nm: 1_000_000, text_thickness_nm: 150_000, italic: false, upright: true }, copper: { line_width_nm: 250_000, text_width_nm: 1_500_000, text_height_nm: 1_500_000, text_thickness_nm: 300_000, italic: false, upright: true }, edge_cuts_line_width_nm: 100_000, courtyard_line_width_nm: 60_000, fab: { line_width_nm: 120_000, text_width_nm: 1_000_000, text_height_nm: 1_000_000, text_thickness_nm: 150_000, italic: false, upright: true }, others: { line_width_nm: 110_000, text_width_nm: 1_000_000, text_height_nm: 1_000_000, text_thickness_nm: 150_000, italic: false, upright: true } };
  assert.deepEqual(["F.SilkS", "F.Cu", "Edge.Cuts", "F.CrtYd", "F.Fab", "Dwgs.User"].map((l) => fpStrokeWidthNm(l, defaults)), [150_000, 250_000, 100_000, 60_000, 120_000, 110_000]);
  assert.equal(fpShape("segment", [[0, 0], [1_000_000, 0]], "F.SilkS", defaults)?.stroke_width, 150_000);
});
