import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin, Schematic } from "../api/types";
import { boxPins, describePin, hitPin, parsePinItemId, pickPin, pinBox, pinById, pinDistance, pinItemId, pinItems, pinsOf, referenceOfKey, selectedPins, symbolKey } from "./schPins";

// 0.6 of the size per character: wide enough to see a name box, no font needed
const measure = (text: string, size: number) => text.length * size * 0.6;

const pin = (number: string, y: number, angle: number, extra: Partial<LibPin> = {}): LibPin => ({ number, name: `P${number}`, electrical_type: "passive", shape: "line", at: [0, y], angle_deg: angle, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, ...extra });

// A two-pin resistor R1 at (50, 50) mm, pin 1 above the body and pin 2 below it, a second unit of U2 whose pins are numbered 5 and 6.
function sheet(extra: Record<string, unknown> = {}): Schematic {
  const r = {
    id: "R1",
    lib_id: "Dev:R",
    at: [50_000, 50_000],
    rot: 0,
    mirror: null,
    unit: 1,
    body_style: 1,
    value: null,
    mpn: null,
    package: null,
    footprint: null,
    datasheet: null,
    pins: [
      { number: "1", name: "P1", kind: "passive", net: "A" },
      { number: "2", name: "P2", kind: "passive", net: "B" },
    ],
    ...extra,
  };
  return {
    lib_symbols: { "Dev:R": { graphics: [], pins: [pin("1", 3.81, 270), pin("2", -3.81, 90), pin("3", 0, 0, { hidden: true })], pin_name_offset: 0 } },
    symbols: [r],
    power_symbols: [],
    wires: [],
    no_connects: [],
    labels: [],
    texts: [],
    title_block: null,
    bus_entries: [],
    sheets: [],
    sheet_path: [],
  } as unknown as Schematic;
}

test("a pin's id is the symbol's key and its number, and reads back", () => {
  assert.equal(symbolKey("U1", 1), "U1");
  assert.equal(symbolKey("U1", 2), "U1#2");
  assert.equal(pinItemId("U1#2", "A3"), "pin:U1#2:A3");
  assert.deepEqual(parsePinItemId("pin:U1#2:A3"), { key: "U1#2", number: "A3" });
  // a number may hold a colon: the key ends at the first one
  assert.deepEqual(parsePinItemId("pin:R1:A:1"), { key: "R1", number: "A:1" });
  assert.equal(parsePinItemId("R1"), null);
  assert.equal(parsePinItemId("fld:R1:Reference"), null);
  assert.equal(parsePinItemId("pin:R1:"), null);
  assert.equal(parsePinItemId("pin::3"), null);
  assert.equal(referenceOfKey("U1#2"), "U1");
  assert.equal(referenceOfKey("R1"), "R1");
});

test("every visible pin of a placed symbol is an item, where the symbol puts it, on the net the sheet says; a hidden pin is not", () => {
  const items = pinItems(sheet());
  assert.deepEqual(items.map((p) => p.id), ["pin:R1:1", "pin:R1:2"], "pin 3 is hidden");
  const p1 = pinById(sheet(), "pin:R1:1")!;
  // the library's y runs up: pin 1 is 3.81 mm above the origin
  assert.deepEqual(p1.tip, [50_000, 46_190]);
  assert.deepEqual(p1.root, [50_000, 48_730]);
  assert.equal(p1.net, "A");
  assert.equal(pinById(sheet(), "pin:R1:9"), null);
  assert.equal(pinsOf(sheet(), "R1").length, 2);
  assert.equal(pinsOf(sheet(), "R1", 2).length, 0);
});

test("the places Swap Pins traded move the pins and keep their numbers and nets", () => {
  const swapped = sheet({ pin_swaps: { "1": "2", "2": "1" } });
  const p1 = pinById(swapped, "pin:R1:1")!;
  const p2 = pinById(swapped, "pin:R1:2")!;
  assert.deepEqual(p1.tip, [50_000, 53_810], "pin 1 is where pin 2 was");
  assert.deepEqual(p2.tip, [50_000, 46_190]);
  assert.equal(p1.net, "A", "the net is the sheet's to say");
  // the direction came along: pin 1 now points up from below the body
  assert.deepEqual(p1.root, [50_000, 51_270]);
});

test("a hit is the pin's line, its name or its number, with at least a quarter of the pin symbol size to spare", () => {
  const p1 = pinById(sheet(), "pin:R1:1")!;
  const box = pinBox(p1, measure);
  assert.ok(box.minY <= 46_190 && box.maxY >= 48_730, "the box holds the line");
  // on the line, in the middle
  assert.ok(hitPin(p1, [50_000, 47_460], 0, measure));
  // a little beside it: 100 um is inside the floor (158.75 um), 400 um is not
  assert.ok(hitPin(p1, [50_100, 47_460], 0, measure));
  assert.ok(!hitPin(p1, [50_000 + 3_000, 47_460], 0, measure));
  assert.ok(hitPin(p1, [50_000 + 3_000, 47_460], 2_000, measure), "a larger accuracy reaches it");
  // the number is written beside the line (the name is over it, the symbol's names are not inside the body): a point on it is a hit too
  assert.ok(hitPin(p1, [50_990, 47_460], 0, measure));
  assert.equal(pinDistance(p1, [50_000, 47_460], measure), 0);
  assert.ok(pinDistance(p1, [60_000, 47_460], measure) > 8_000);
});

test("an exact hit on a pin wins; a near one is picked only when asked", () => {
  const sch = sheet();
  assert.equal(pickPin(sch, [50_000, 47_460], 300, measure, true)?.id, "pin:R1:1");
  // 3 mm from the line, no text there: not exact
  assert.equal(pickPin(sch, [50_000 + 3_000, 47_460], 300, measure, true), null);
  assert.equal(pickPin(sch, [50_000, 49_300], 1_200, measure, false)?.id, "pin:R1:1", "within a 1.2 mm pick radius of the end of the line");
  assert.equal(pickPin(sch, [90_000, 90_000], 1_200, measure, false), null);
  // the selection filter keeps a pin out
  assert.equal(pickPin(sch, [50_000, 47_460], 300, measure, true, (id) => id !== "pin:R1:1"), null);
});

test("a box takes the pins whose boxes it touches (crossing) or holds (window)", () => {
  const sch = sheet();
  assert.deepEqual(boxPins(sch, { minX: 49_000, minY: 46_000, maxX: 51_000, maxY: 49_000 }, true, measure), ["pin:R1:1"]);
  assert.deepEqual(boxPins(sch, { minX: 49_900, minY: 47_000, maxX: 50_100, maxY: 47_500 }, false, measure), [], "a window that holds none of the pin's box");
  assert.deepEqual(boxPins(sch, { minX: 40_000, minY: 40_000, maxX: 60_000, maxY: 60_000 }, false, measure).sort(), ["pin:R1:1", "pin:R1:2"]);
});

test("a pin is described the way KiCad describes it", () => {
  assert.equal(describePin(pinById(sheet(), "pin:R1:1")!), "Symbol R1 Pin 1 [P1, Passive, Line]");
  const noName = pinById(sheet({}), "pin:R1:1")!;
  assert.equal(describePin({ ...noName, name: "~" }), "Symbol R1 Pin 1 [Passive, Line]");
});

test("selectedPins keeps the order the pins were picked in and drops what is no pin", () => {
  const ids = selectedPins(sheet(), ["pin:R1:2", "R1", "pin:R1:1", "pin:R1:9"]).map((p) => p.id);
  assert.deepEqual(ids, ["pin:R1:2", "pin:R1:1"]);
});
