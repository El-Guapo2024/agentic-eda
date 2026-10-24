import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin, Schematic } from "../api/types";
import { labelOrientationForPoint, orientationSceneOf, pinSpinStyle, type OrientationScene, type Spin } from "./schAutoRotate";

const scene = (extra: Partial<OrientationScene> = {}): OrientationScene => ({ lines: [], busEntries: [], symbols: [], ...extra });
const line = (a: [number, number], b: [number, number], bus = false) => ({ a, b, bus });

test("a label at the end of a wire reads on along it, at the start the other way (GetLabelOrientationForPoint, SCH_LINE_T)", () => {
  const s = scene({ lines: [line([0, 0], [10_160_000, 0]), line([30_480_000, 0], [30_480_000, 10_160_000])] });
  assert.equal(labelOrientationForPoint(s, [10_160_000, 0], "up"), "right");
  assert.equal(labelOrientationForPoint(s, [0, 0], "up"), "left");
  // a vertical wire drawn downwards: the end hangs below it, the start goes up
  assert.equal(labelOrientationForPoint(s, [30_480_000, 10_160_000], "up"), "bottom");
  assert.equal(labelOrientationForPoint(s, [30_480_000, 0], "right"), "up");
  // drawn the other way round, the same pictures: it is the end the label is at that counts
  const r = scene({ lines: [line([10_160_000, 0], [0, 0]), line([30_480_000, 10_160_000], [30_480_000, 0])] });
  assert.equal(labelOrientationForPoint(r, [10_160_000, 0], "up"), "right");
  assert.equal(labelOrientationForPoint(r, [0, 0], "up"), "left");
  assert.equal(labelOrientationForPoint(r, [30_480_000, 10_160_000], "up"), "bottom");
  assert.equal(labelOrientationForPoint(r, [30_480_000, 0], "right"), "up");
});

test("a label in the middle of a wire gets what the C++ gives it: not the end, so the 'start' answer", () => {
  const s = scene({ lines: [line([0, 0], [10_160_000, 0])] });
  assert.equal(labelOrientationForPoint(s, [5_080_000, 0], "up"), "left");
});

test("a label nothing overlaps keeps the spin it has", () => {
  const s = scene({ lines: [line([0, 0], [10_160_000, 0])] });
  assert.equal(labelOrientationForPoint(s, [50_800_000, 50_800_000], "bottom"), "bottom");
  assert.equal(labelOrientationForPoint(s, [50_800_000, 50_800_000], "left"), "left");
});

test("a diagonal line is horizontal up to 45 degrees (-45 < angle <= 45) and vertical beyond", () => {
  const s = scene({ lines: [line([0, 0], [5_080_000, 5_080_000]), line([20_320_000, 0], [22_860_000, 5_080_000]), line([40_640_000, 5_080_000], [45_720_000, 0])] });
  assert.equal(labelOrientationForPoint(s, [5_080_000, 5_080_000], "up"), "right", "45 degrees down-right is horizontal");
  assert.equal(labelOrientationForPoint(s, [22_860_000, 5_080_000], "up"), "bottom", "steeper is vertical");
  assert.equal(labelOrientationForPoint(s, [45_720_000, 0], "up"), "up", "-45 is not above -45: vertical, and the end of an upward line is its top");
});

test("a line counts wherever its bounding box (the ends grown by half its pen) holds the point, as the R-tree's Overlapping( aPosition ) does", () => {
  const s = scene({ lines: [line([0, 0], [10_000_000, 10_000_000])] });
  assert.equal(labelOrientationForPoint(s, [5_000_000, 2_000_000], "right"), "left", "inside a diagonal line's box: 45 degrees is horizontal, not the end");
  const w = scene({ lines: [line([0, 0], [10_160_000, 0])] });
  assert.equal(labelOrientationForPoint(w, [5_080_000, 70_000], "up"), "left", "within half of a wire's 6 mil");
  assert.equal(labelOrientationForPoint(w, [5_080_000, 200_000], "up"), "up", "beyond it");
});

test("a pin asks for the spin that points away from its symbol's body, and wins over a wire that ends on it (symbols come after lines)", () => {
  assert.equal(pinSpinStyle([0, 1]), "up", "the body is below the pin: the label reads up");
  assert.equal(pinSpinStyle([0, -1]), "bottom");
  assert.equal(pinSpinStyle([1, 0]), "left");
  assert.equal(pinSpinStyle([-1, 0]), "right");
  assert.equal(pinSpinStyle([0, 0]), null, "a pin with no direction asks for nothing");
  const s = scene({ lines: [line([10_160_000, 21_590_000], [20_320_000, 21_590_000])], symbols: [[{ at: [20_320_000, 21_590_000], body: [0, 1] }]] });
  assert.equal(labelOrientationForPoint(s, [20_320_000, 21_590_000], "left"), "up");
  // only the first pin of a symbol that is on the point counts
  const two = scene({ symbols: [[{ at: [0, 0], body: [0, 1] }, { at: [0, 0], body: [1, 0] }]] });
  assert.equal(labelOrientationForPoint(two, [0, 0], "right"), "up");
});

test("a label at a bus entry goes to the side the entry is on, for a bus drawn downwards or to the right (Angle() == 90 or 0)", () => {
  const vertical = scene({ lines: [line([0, 0], [0, 20_320_000], true)], busEntries: [{ at: [0, 5_080_000], size: [2_540_000, 2_540_000] }, { at: [0, 12_700_000], size: [-2_540_000, 2_540_000] }] });
  assert.equal(labelOrientationForPoint(vertical, [2_540_000, 7_620_000], "up"), "right");
  assert.equal(labelOrientationForPoint(vertical, [-2_540_000, 15_240_000], "up"), "left");
  const horizontal = scene({ lines: [line([0, 0], [20_320_000, 0], true)], busEntries: [{ at: [5_080_000, 0], size: [2_540_000, -2_540_000] }, { at: [12_700_000, 0], size: [2_540_000, 2_540_000] }] });
  assert.equal(labelOrientationForPoint(horizontal, [7_620_000, -2_540_000], "right"), "up");
  assert.equal(labelOrientationForPoint(horizontal, [15_240_000, 2_540_000], "right"), "bottom");
  // a vertical bus drawn upwards is -90 degrees and changes nothing
  const up = scene({ lines: [line([0, 20_320_000], [0, 0], true)], busEntries: [{ at: [0, 5_080_000], size: [2_540_000, 2_540_000] }] });
  assert.equal(labelOrientationForPoint(up, [2_540_000, 7_620_000], "up"), "up");
});

// ------------------------------------------------------------------------------------------------------------------------------------- from a sheet

const pin = (number: string, y: number, angle: number, extra: Partial<LibPin> = {}): LibPin => ({ number, name: `P${number}`, electrical_type: "passive", shape: "line", at: [0, y], angle_deg: angle, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, ...extra });

function sheet(rot: number, mirror: "x" | "y" | null): Schematic {
  const r = { id: "R1", lib_id: "Dev:R", at: [50_000_000, 50_000_000], rot, mirror, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] };
  const gnd = { id: "#PWR01", lib_id: "power:GND", at: [80_000_000, 80_000_000], rot: 0, net: "GND", pin: { number: "1", name: "GND", kind: "power" } };
  return {
    lib_symbols: {
      // pin 1 above the body (it runs down into it), pin 2 below; the power symbol's pin is hidden and has no length
      "Dev:R": { graphics: [], pins: [pin("1", 3.81, 270), pin("2", -3.81, 90)], pin_name_offset: 0 },
      "power:GND": { graphics: [], pins: [pin("1", 0, 270, { length_mm: 0, hidden: true })], pin_name_offset: 0 },
    },
    symbols: [r],
    power_symbols: [gnd],
    wires: [],
    lines: [],
    no_connects: [],
    labels: [],
    texts: [],
    title_block: null,
    bus_entries: [],
    sheets: [],
    sheet_path: [],
  } as unknown as Schematic;
}

test("a resistor's top pin asks for Up and its bottom pin for Bottom; a power symbol's pin, which has no length, asks for the way its angle runs", () => {
  const s = orientationSceneOf(sheet(0, null));
  assert.equal(labelOrientationForPoint(s, [50_000_000, 46_190_000], "right"), "up");
  assert.equal(labelOrientationForPoint(s, [50_000_000, 53_810_000], "right"), "bottom");
  // power:GND hangs down from its pin: the pin line runs down into the body, so a label there reads up
  assert.equal(labelOrientationForPoint(s, [80_000_000, 80_000_000], "right"), "up");
});

test("in every orientation the label on a resistor's pin reads away from the other pin", () => {
  const away = (from: readonly [number, number], to: readonly [number, number]): Spin => {
    const dx = to[0] - from[0];
    const dy = to[1] - from[1];
    return Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? "right" : "left") : dy > 0 ? "bottom" : "up";
  };
  for (const rot of [0, 90, 180, 270]) {
    for (const mirror of [null, "x", "y"] as const) {
      const sch = sheet(rot, mirror);
      const s = orientationSceneOf(sch);
      const [a, b] = s.symbols[0]!;
      assert.equal(labelOrientationForPoint(s, a!.at, "right"), away(b!.at, a!.at), `rot ${rot} mirror ${mirror} pin 1`);
      assert.equal(labelOrientationForPoint(s, b!.at, "right"), away(a!.at, b!.at), `rot ${rot} mirror ${mirror} pin 2`);
    }
  }
});

test("the places Swap Pins traded carry their direction with them", () => {
  const sch = sheet(0, null);
  (sch.symbols[0] as unknown as { pin_swaps: Record<string, string> }).pin_swaps = { "1": "2", "2": "1" };
  const s = orientationSceneOf(sch);
  // pin 1 is where pin 2 was, below the body, so its label reads down
  const [p1] = s.symbols[0]!;
  assert.deepEqual(p1!.at, [50_000_000, 53_810_000]);
  assert.equal(labelOrientationForPoint(s, p1!.at, "right"), "bottom");
});

test("wires, graphic lines and bus entries of a sheet make the scene's lines and entries", () => {
  const sch = sheet(0, null);
  (sch as unknown as { wires: unknown }).wires = [{ id: "w", net: "N", pins: [], pts: [[0, 0], [10_160_000, 0], [10_160_000, 5_080_000]], bus: false }, { id: "b", net: "", pins: [], pts: [[0, 20_000_000], [0, 30_000_000]], bus: true }];
  (sch as unknown as { lines: unknown }).lines = [{ id: "l", pts: [[1, 1], [2000, 2000]], width_nm: 0 }];
  (sch as unknown as { bus_entries: unknown }).bus_entries = [{ id: "e", at: [0, 22_000_000], size: [2_540_000, 2_540_000] }];
  const s = orientationSceneOf(sch);
  assert.deepEqual(s.lines.map((l) => [l.a, l.b, l.bus]), [[[0, 0], [10_160_000, 0], false], [[10_160_000, 0], [10_160_000, 5_080_000], false], [[0, 20_000_000], [0, 30_000_000], true], [[1, 1], [2000, 2000], false]]);
  assert.deepEqual(s.busEntries, [{ at: [0, 22_000_000], size: [2_540_000, 2_540_000] }]);
  // the end of the wire's last segment hangs below it
  assert.equal(labelOrientationForPoint(s, [10_160_000, 5_080_000], "up"), "bottom");
  // a label on that bus entry's far end goes to the right of the bus
  assert.equal(labelOrientationForPoint(s, [2_540_000, 24_540_000], "up"), "right");
});
