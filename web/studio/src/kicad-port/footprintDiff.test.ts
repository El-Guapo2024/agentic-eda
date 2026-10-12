import { test } from "node:test";
import assert from "node:assert/strict";
import type { FootprintDiff, FootprintDrawing } from "../api/fpAssign";
import {
  arcPath,
  diffOpacity,
  drawingBounds,
  layerRank,
  padFigure,
  shapePath,
  summaryLines,
  toggleSlider,
} from "./footprintDiff";

const diff = (extra: Partial<FootprintDiff>): FootprintDiff => ({
  ok: true,
  reference: "U1",
  library: "Alt",
  item: "WIDE",
  found: true,
  ...extra,
});
const pad = (
  extra: Partial<FootprintDrawing["pads"][number]> = {},
): FootprintDrawing["pads"][number] => ({
  num: "1",
  x: 0,
  y: 0,
  w: 1000,
  h: 600,
  shape: "rect",
  rot: 0,
  ratio: null,
  drill: null,
  slot: null,
  th: false,
  ...extra,
});
const drawing = (extra: Partial<FootprintDrawing>): FootprintDrawing => ({
  pads: [],
  shapes: [],
  courtyard: [],
  ...extra,
});

test("the summary page names the footprint, the library and the item, then what differs", () => {
  const lines = summaryLines(
    diff({ lines: ["Pad 1 size differs on layer F.Cu."] }),
  );
  assert.deepEqual(
    lines.map((l) => l.text),
    [
      "Board vs library diff for:",
      "    Footprint U1",
      "    Library: Alt",
      "    Library item: WIDE",
      "",
      "Pad 1 size differs on layer F.Cu.",
    ],
  );
});

test("no differences is said in KiCad's words, and a missing library copy says why", () => {
  assert.equal(
    summaryLines(diff({ lines: [] })).at(-1)!.text,
    "No relevant differences detected.",
  );
  const missing = summaryLines(
    diff({
      found: false,
      reason: "The library no longer contains the item WIDE.",
    }),
  );
  assert.equal(
    missing.at(-1)!.text,
    "The library no longer contains the item WIDE.",
  );
  assert.equal(missing.at(-1)!.severity, "warning");
  assert.equal(
    summaryLines(diff({ reference: "" }))[1]!.text,
    "    Footprint <no reference designator>",
  );
});

test("the slider fades one footprint out as it fades the other in, both whole in the middle", () => {
  assert.deepEqual(diffOpacity(0), { board: 1, library: 0 });
  assert.deepEqual(diffOpacity(50), { board: 1, library: 1 });
  assert.deepEqual(diffOpacity(100), { board: 0, library: 1 });
  assert.deepEqual(diffOpacity(25), { board: 1, library: 0.5 });
  assert.deepEqual(diffOpacity(75), { board: 0.5, library: 1 });
  assert.equal(toggleSlider(0), 100);
  assert.equal(toggleSlider(100), 0);
  assert.equal(toggleSlider(50), 0, "from anywhere else to the board's");
});

test("the bounds hold the pads at their rotation, the strokes with their width and the courtyard", () => {
  assert.equal(drawingBounds([drawing({})]), null);
  const d = drawing({
    pads: [pad({ x: -2000, w: 1000, h: 600 }), pad({ x: 2000, rot: 90 })],
    shapes: [
      {
        id: "s",
        kind: "segment",
        layer: "F.SilkS",
        stroke_width: 120,
        filled: false,
        start: [-3000, -1000],
        end: [3000, -1000],
      },
    ],
    courtyard: [
      [
        [-3500, -1500],
        [3500, 1500],
      ],
    ],
  });
  const b = drawingBounds([d])!;
  assert.equal(b.x0, -3500);
  assert.equal(b.x1, 3500);
  assert.equal(b.y0, -1500);
  assert.equal(b.y1, 1500);
  const rotated = drawingBounds([drawing({ pads: [pad({ rot: 90 })] })])!;
  assert.ok(
    Math.abs(rotated.x1 - 300) < 1e-6 && Math.abs(rotated.y1 - 500) < 1e-6,
    "a pad turned a quarter has its width and height swapped",
  );
  const circle = drawingBounds([
    drawing({
      shapes: [
        {
          id: "c",
          kind: "circle",
          layer: "F.Fab",
          stroke_width: 100,
          filled: false,
          center: [0, 0],
          end: [1000, 0],
        },
      ],
    }),
  ])!;
  assert.equal(circle.x1, 1050);
});

test("an arc through three points is the arc SVG draws: the small side and the large side, either way round", () => {
  // a quarter of the unit circle, counter-clockwise in the numbers (increasing angle)
  assert.equal(
    arcPath([1000, 0], [Math.SQRT1_2 * 1000, Math.SQRT1_2 * 1000], [0, 1000]),
    "M 1000 0 A 1000 1000 0 0 1 0 1000",
  );
  // the same quarter the other way: the sweep flag turns
  assert.equal(
    arcPath([0, 1000], [Math.SQRT1_2 * 1000, Math.SQRT1_2 * 1000], [1000, 0]),
    "M 0 1000 A 1000 1000 0 0 0 1000 0",
  );
  // three quarters: the large-arc flag
  assert.equal(
    arcPath([1000, 0], [-1000, 0], [0, -1000]),
    "M 1000 0 A 1000 1000 0 1 1 0 -1000",
    "through the far side of the circle, 270 degrees",
  );
  assert.equal(
    arcPath([0, 0], [500, 0], [1000, 0]),
    "M 0 0 L 1000 0",
    "three points on a line are a line",
  );
});

test("graphics are paths: strokes stay strokes, a filled polygon is solid", () => {
  assert.deepEqual(
    shapePath({
      id: "a",
      kind: "segment",
      layer: "F.SilkS",
      stroke_width: 120,
      filled: true,
      start: [0, 0],
      end: [10, 20],
    }),
    { d: "M 0 0 L 10 20", fill: false, width: 120, layer: "F.SilkS" },
  );
  const rect = shapePath({
    id: "r",
    kind: "rect",
    layer: "F.Fab",
    stroke_width: 100,
    filled: false,
    start: [0, 0],
    end: [10, 20],
  });
  assert.equal(rect.d, "M 0 0 L 10 0 L 10 20 L 0 20 Z");
  const poly = shapePath({
    id: "p",
    kind: "polygon",
    layer: "F.Fab",
    stroke_width: 0,
    filled: true,
    pts: [
      [0, 0],
      [10, 0],
      [0, 10],
    ],
  });
  assert.deepEqual([poly.d, poly.fill], ["M 0 0 L 10 0 L 0 10 Z", true]);
  assert.equal(
    shapePath({
      id: "b",
      kind: "bezier",
      layer: "F.SilkS",
      stroke_width: 100,
      filled: false,
      start: [0, 0],
      c1: [1, 1],
      c2: [2, 1],
      end: [3, 0],
    }).d,
    "M 0 0 C 1 1 2 1 3 0",
  );
  assert.match(
    shapePath({
      id: "c",
      kind: "circle",
      layer: "F.Fab",
      stroke_width: 100,
      filled: false,
      center: [0, 0],
      end: [500, 0],
    }).d,
    /^M 500 0 A 500 500 0 1 0 -500 0/,
  );
});

test("a pad is a rectangle, a pill, a rounded rectangle or an ellipse, with its drill", () => {
  assert.deepEqual(
    [
      padFigure(pad({ shape: "circle", w: 800, h: 800 })).shape,
      padFigure(pad({ shape: "circle" })).radius,
    ],
    ["ellipse", 0],
  );
  assert.equal(padFigure(pad({ shape: "oval" })).radius, 300);
  assert.equal(
    padFigure(pad({ shape: "round_rect" })).radius,
    150,
    "a quarter of the short side when the file does not say",
  );
  assert.equal(padFigure(pad({ shape: "round_rect", ratio: 0.5 })).radius, 300);
  assert.equal(padFigure(pad({ shape: "trapezoid" })).radius, 0);
  assert.deepEqual(padFigure(pad({ th: true, drill: 800 })).hole, {
    w: 800,
    h: 800,
  });
  assert.deepEqual(
    padFigure(pad({ th: true, drill: 800, slot: [1600, 800] })).hole,
    { w: 1600, h: 800 },
  );
  assert.equal(padFigure(pad()).hole, null);
});

test("documentation is painted under the copper and the silkscreen over it", () => {
  const layers = ["F.SilkS", "F.Cu", "F.CrtYd", "F.Fab", "Cmts.User"];
  assert.deepEqual(
    [...layers].sort((a, b) => layerRank(a) - layerRank(b)),
    ["F.Fab", "F.CrtYd", "F.Cu", "F.SilkS", "Cmts.User"],
  );
});
