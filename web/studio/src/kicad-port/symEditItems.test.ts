import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import { deletePlan, duplicateItems, maxPinNumber, nearestHalfGrid, pastedItems, placedPinWithId, repeatPin, rotatePivot, rotatePlan, selectionPivot } from "./symEditItems";
import { pinBoxAt } from "./schPins";
import { resolvePin, symbolTransformMatrix } from "../components/schematic/transform";

const pin = (id: string, number: string, x: number, y: number, unit = 1, extra: Partial<LibrarySymbolPin> = {}): LibrarySymbolPin => ({
  id,
  number,
  name: "A",
  electrical_type: "input",
  shape: "line",
  at: { x, y },
  angle_deg: 0,
  length_mm: 2.54,
  unit,
  body_style: 1,
  hidden: false,
  name_size_mm: 1.27,
  number_size_mm: 1.27,
  ...extra,
});
const rect: LibrarySymbolGraphic = { kind: "rectangle", id: "g1", unit: 1, body_style: 1, start: { x: -1, y: -1 }, end: { x: 1, y: 1 }, stroke_mm: 0.254, fill: "none" };

test("DoDelete in sync mode takes the twin of each pin in the other units, one per unit (E4)", () => {
  const pins = [pin("a", "1", 0, 0, 1), pin("b", "1-UB", 0, 0, 2), pin("c", "1-UC", 0, 0, 3), pin("c2", "9", 0, 0, 3), pin("d", "2", 0, 0, 2, { name: "B" })];
  const plan = deletePlan(["a", "g1"], pins, [rect], true, 3);
  assert.deepEqual(plan.pins.sort(), ["a", "b", "c"], "the twins at the same place with the same name, type and body style -- the first per unit; another name stays");
  assert.deepEqual(plan.graphics, ["g1"]);
  assert.deepEqual(deletePlan(["a"], pins, [], false, 3).pins, ["a"], "out of sync mode only the selection");
});

test("Rotate turns several pins about the half-grid centre of the selection, one pin about itself (E5)", () => {
  const p1 = pin("a", "1", 0, 0);
  const p2 = pin("b", "2", 0, -2.54);
  // the box of both pins (their ends at x 2.54): x 0..2.54, y -2.54..0 -> centre (1.27, -1.27), on the 0.635 half grid of 1.27
  assert.deepEqual(nearestHalfGrid({ x: 1.3, y: -1.2 }, 1.27), { x: 1.27, y: -1.27 });
  const [r1, r2] = rotatePlan([p1, p2], 2, true, 1.27);
  assert.deepEqual([r1!.at, r1!.angle_deg], [{ x: 0, y: -2.54 }, 90]);
  assert.deepEqual([r2!.at, r2!.angle_deg], [{ x: 2.54, y: -2.54 }, 90]);
  const [one] = rotatePlan([p2], 1, false, 1.27);
  assert.deepEqual([one!.at, one!.angle_deg], [{ x: 0, y: -2.54 }, 270], "one item turns in place, clockwise");
});

test("RepeatPin: 100 mil down for a horizontal pin, across for a vertical one, name and number incremented (E3)", () => {
  const r = repeatPin(pin("a", "1", 0, 0, 1, { name: "A1" }));
  assert.deepEqual([r.number, r.name, r.at, r.id], ["2", "A2", { x: 0, y: -2.54 }, undefined]);
  const up = repeatPin(pin("a", "B9", 5.08, 0, 1, { name: "CLK", angle_deg: 270 }));
  assert.deepEqual([up.number, up.name, up.at], ["B10", "CLK", { x: 7.62, y: 0 }]);
});

test("Duplicate numbers each copied pin GetMaxPinNumber() + 1, in the order of the old numbers (E3)", () => {
  const pins = [pin("a", "3", 0, 0), pin("b", "1", 0, -2.54), pin("c", "", 0, -5.08), pin("d", "X", 0, -7.62)];
  assert.equal(maxPinNumber(pins), 3);
  const d = duplicateItems(["a", "b", "c", "g1"], pins, [rect]);
  assert.deepEqual(d.pins.map((p) => p.number), ["4", "5", ""], "1 then 3 (StrNumCmp), the unnumbered pin keeps no number");
  assert.equal(d.graphics.length, 1);
  assert.ok(d.pins.every((p) => p.id === undefined) && d.graphics[0]!.id === undefined);
});

test("Paste puts the items on the unit and body style being edited unless common to all", () => {
  const out = pastedItems({ pins: [pin("a", "1", 0, 0, 1), pin("b", "2", 0, 0, 0, { body_style: 0 })], graphics: [rect] }, 3, 2);
  assert.deepEqual(out.pins.map((p) => [p.unit, p.body_style]), [[3, 2], [0, 0]]);
  assert.deepEqual([out.graphics[0]!.unit, out.graphics[0]!.body_style], [3, 2]);
});

test("Repeat Pin keeps the last pin by its id (g_lastPin is a UUID), so it is found after an edit (r3 E3-4)", () => {
  const placed = { ...pin("", "1", 0, 0), id: undefined };
  const after = [pin("p0", "3", 5, 5), pin("p7", "1", 0, 0), pin("p8", "1", 0, 0, 2)];
  const last = placedPinWithId(placed, after);
  assert.equal(last.id, "p7", "the pin of the symbol that is the pin as placed, on its unit");
  // Renumbered to 5 since: found by id, and repeated as it is now.
  const now = [pin("p7", "5", 0, 0)];
  const source = now.find((p) => p.id === last.id)!;
  assert.deepEqual([repeatPin(source).number, repeatPin(source).at], ["6", { x: 0, y: -2.54 }]);
  assert.equal(placedPinWithId(placed, []).id, undefined);
});

test("Rotate's pivot counts the pins' name and number boxes (PIN_LAYOUT_CACHE::GetPinBoundingBox), KiROUNDed in y-down IU (r3 E3-3)", () => {
  const measure = (text: string, size: number) => text.length * size * 0.6;
  const texts = { namesHidden: false, numbersHidden: false, nameOffsetNm: 508_000 };
  const pins = [pin("a", "1", 0, 0, 1, { name: "IN" }), pin("b", "2", 0, -2.54, 1, { name: "OUT" })];
  const boxes = pins.map((p) => {
    const r = resolvePin({ number: p.number, name: p.name, electrical_type: p.electrical_type, shape: p.shape, at: [p.at.x, p.at.y], angle_deg: p.angle_deg, length_mm: p.length_mm, unit: 1, body_style: 1, hidden: false }, symbolTransformMatrix(0, null), [0, 0]);
    return pinBoxAt(r.tip, r.root, p.name, p.number, texts, measure, 1.27e6, 1.27e6);
  });
  const c = selectionPivot(boxes, 1.27)!;
  assert.equal(c.x, 2.54, "the names inside the body widen the box to the right; the ends alone give 1.27");
  // y: the number of the upper pin over its line (1.625 up) and the name of the lower one on its line (3.175 down): centre -0.775, on the half grid -0.635
  assert.equal(c.y, -0.635);
  const [r1] = rotatePlan(pins, 2, true, 1.27, [], c);
  assert.deepEqual(r1!.at, { x: 1.905, y: -3.175 });
});

test("Rotate's pivot counts a body_style 1 rectangle and skips a text, as SELECTION::GetCenter (r4 E4-1)", () => {
  const rect: LibrarySymbolGraphic = { id: "r", kind: "rectangle", unit: 0, body_style: 1, start: { x: 0, y: 0 }, end: { x: 10, y: 10 } } as unknown as LibrarySymbolGraphic;
  const text: LibrarySymbolGraphic = { id: "t", kind: "text", unit: 0, body_style: 1, at: { x: 20, y: 20 }, text: "X", size_mm: 1.27 } as unknown as LibrarySymbolGraphic;
  // a pin's box at (0, 5) pointing left, in y-down nm
  const pinBox = { minX: -2_540_000, minY: -5_000_000, maxX: 0, maxY: -5_000_000 };
  // `graphicBox` of itemBoxes.ts in the editor's y-down nm (a text's box would be around (20, -20))
  const boxOf = (g: LibrarySymbolGraphic) => (g.kind === "rectangle" ? { minX: 0, minY: -10_000_000, maxX: 10_000_000, maxY: 0 } : { minX: 19e6, minY: -21e6, maxX: 21e6, maxY: -19e6 });
  const c = rotatePivot([pinBox], [rect, text], 1.27, boxOf)!;
  // box x -2.54..10 (centre 3.73 -> 3.81 on the half grid), y 0..10 (centre 5 -> 5.08); the text at (20, 20) is not in it
  assert.deepEqual(c, { x: 3.81, y: 5.08 });
});

test("Rotate's pivot of texts alone is the mean of their positions (SELECTION::GetCenter hasOnlyText)", () => {
  const t = (id: string, x: number, y: number) => ({ id, kind: "text", unit: 1, body_style: 2, at: { x, y }, text: "A", size_mm: 1.27 }) as unknown as LibrarySymbolGraphic;
  assert.deepEqual(rotatePivot([], [t("a", 0, 0), t("b", 5.08, 2.54)], 1.27, () => null), { x: 2.54, y: 1.27 });
});
