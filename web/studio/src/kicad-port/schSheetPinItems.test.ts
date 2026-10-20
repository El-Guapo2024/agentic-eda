import { test } from "node:test";
import assert from "node:assert/strict";
import type { Schematic } from "../api/types";
import { boxSheetPins, hitSheetPin, sheetPinBodyBox, sheetPinBox, sheetPinById, sheetPinItems, sheetPinSide } from "./schSheetPinItems";
import { constrainOnEdge, sheetPinEditCmd } from "./schSheetPins";

const measure = (text: string, size: number) => text.length * size * 0.6;

// A sheet at (100, 50) mm, 40 x 30 mm, with a pin on each border.
const sheet = { id: "sheet_1", name: "Sub", file: "sub.kicad_sch", at: [100_000, 50_000] as [number, number], size: [40_000, 30_000] as [number, number] };
const sch = {
  sheets: [
    {
      ...sheet,
      pins: [
        { id: "shpin_l", name: "IN", shape: "input", at: [100_000, 60_000] },
        { id: "shpin_r", name: "OUT", shape: "output", at: [140_000, 70_000] },
        { id: "shpin_t", name: "CLK", shape: "input", at: [120_000, 50_000] },
        { id: "shpin_b", name: "RST", shape: "input", at: [130_000, 80_000] },
      ],
    },
  ],
} as unknown as Schematic;

test("a pin is on the border it sits at: left, right, top, then bottom", () => {
  assert.equal(sheetPinSide(sheet, [100_000, 60_000]), "left");
  assert.equal(sheetPinSide(sheet, [140_000, 70_000]), "right");
  assert.equal(sheetPinSide(sheet, [120_000, 50_000]), "top");
  assert.equal(sheetPinSide(sheet, [130_000, 80_000]), "bottom");
  // the corner belongs to the vertical border, as the writer and the painter have it
  assert.equal(sheetPinSide(sheet, [100_000, 50_000]), "left");
});

test("the body box is the flag and the name from the pin's own point, into the sheet (SCH_HIERLABEL::GetBodyBoundingBox)", () => {
  // text 1.27 mm: pen 159, margin 191 (0.15 of the size, rounded) -> height 1620; the name is 4 mm wide -> length 5620
  const left = sheetPinBodyBox([100_000, 60_000], "left", 4_000);
  assert.ok(Math.abs(left.minX - (100_000 - 304.8)) < 0.01 && Math.abs(left.maxX - (100_000 - 304.8 + 5_620)) < 0.01, JSON.stringify(left));
  assert.ok(Math.abs(left.minY - (60_000 - 810)) < 0.01 && Math.abs(left.maxY - (60_000 + 810)) < 0.01);
  // a pin on the right border runs the other way, into the sheet
  const right = sheetPinBodyBox([140_000, 70_000], "right", 4_000);
  assert.ok(right.maxX > 140_000 && right.minX < 140_000 - 5_000);
  // on the top border the box hangs below the pin, on the bottom border it rises above it
  const top = sheetPinBodyBox([120_000, 50_000], "top", 4_000);
  assert.ok(top.minY < 50_000 && top.maxY > 50_000 + 5_000);
  const bottom = sheetPinBodyBox([130_000, 80_000], "bottom", 4_000);
  assert.ok(bottom.maxY > 80_000 && bottom.minY < 80_000 - 5_000);
});

test("every pin of every sheet is an item, found by its id, with the side it is on", () => {
  assert.deepEqual(sheetPinItems(sch).map((p) => [p.id, p.side]), [["shpin_l", "left"], ["shpin_r", "right"], ["shpin_t", "top"], ["shpin_b", "bottom"]]);
  assert.equal(sheetPinById(sch, "shpin_t")?.name, "CLK");
  assert.equal(sheetPinById(sch, "nope"), null);
});

test("a click hits the pin's box and nothing past it", () => {
  const l = sheetPinById(sch, "shpin_l")!;
  assert.ok(hitSheetPin(l, [102_000, 60_000], 0, measure), "on the name, inside the sheet");
  assert.ok(!hitSheetPin(l, [100_000 + 12_000, 60_000], 0, measure), "past the name");
  // "IN" is 1.5 mm wide, so the box ends 3.1 mm from the border
  assert.ok(!hitSheetPin(l, [103_200, 60_000], 0, measure), "just past the box");
  assert.ok(hitSheetPin(l, [103_200, 60_000], 500, measure), "the accuracy reaches a little past it");
  const b = sheetPinBox(l, measure);
  assert.ok(b.minY < 60_000 && b.maxY > 60_000);
});

test("a box takes the pins it touches or holds", () => {
  const around = { minX: 99_000, minY: 59_000, maxX: 108_000, maxY: 61_000 };
  assert.deepEqual(boxSheetPins(sch, around, true, measure), ["shpin_l"]);
  assert.deepEqual(boxSheetPins(sch, around, false, measure), ["shpin_l"], "a window that holds the pin's whole box");
  assert.deepEqual(boxSheetPins(sch, { minX: 100_200, minY: 59_500, maxX: 101_000, maxY: 60_500 }, false, measure), [], "a window that holds only part of it takes nothing");
  assert.deepEqual(boxSheetPins(sch, { minX: 100_200, minY: 59_500, maxX: 101_000, maxY: 60_500 }, true, measure), ["shpin_l"]);
  assert.deepEqual(boxSheetPins(sch, { minX: 0, minY: 0, maxX: 500_000, maxY: 500_000 }, false, measure).sort(), ["shpin_b", "shpin_l", "shpin_r", "shpin_t"]);
});

test("a pin moved along its border stays on it; one dragged past a corner goes to the nearest border (SCH_SHEET_PIN::ConstrainOnEdge)", () => {
  assert.deepEqual(constrainOnEdge({ at: sheet.at, size: sheet.size }, [101_000, 65_000]), { at: [100_000, 65_000], side: "left" });
  // below the bottom-left corner the left border still is the nearest, clamped to the corner
  assert.deepEqual(constrainOnEdge({ at: sheet.at, size: sheet.size }, [100_000, 90_000]).at, [100_000, 80_000]);
  // dragged into the middle of the sheet, near the top
  assert.deepEqual(constrainOnEdge({ at: sheet.at, size: sheet.size }, [120_000, 52_000]), { at: [120_000, 50_000], side: "top" });
});

test("what the dialog sends: the name and the shape that changed, one verb", () => {
  const pin = { id: "shpin_l", name: "IN", shape: "input" as const };
  assert.equal(sheetPinEditCmd(pin, { name: "IN", shape: "input" }), null);
  assert.equal(sheetPinEditCmd(pin, { name: "  IN  ", shape: "input" }), null, "the name is trimmed");
  assert.deepEqual(sheetPinEditCmd(pin, { name: "SCL", shape: "input" }), { op: "sch_edit", verb: "edit_sheet_pin", id: "shpin_l", name: "SCL" });
  assert.deepEqual(sheetPinEditCmd(pin, { name: "IN", shape: "bidirectional" }), { op: "sch_edit", verb: "edit_sheet_pin", id: "shpin_l", shape: "bidirectional" });
  assert.deepEqual(sheetPinEditCmd(pin, { name: "SCL", shape: "output" }), { op: "sch_edit", verb: "edit_sheet_pin", id: "shpin_l", name: "SCL", shape: "output" });
});

test("a pin's text look is set by Sheet Pin Properties: only what changed goes, and the pin's box grows with its size", () => {
  const pin = { id: "shpin_l", name: "IN", shape: "input" as const };
  // the size in micrometres, as the dialog parses it; the default is 50 mil, regular, upright
  assert.equal(sheetPinEditCmd(pin, { name: "IN", shape: "input", sizeUm: 1270, bold: false, italic: false }), null, "the look it has is no change");
  assert.deepEqual(sheetPinEditCmd(pin, { name: "IN", shape: "input", sizeUm: 2000, bold: false, italic: false }), { op: "sch_edit", verb: "edit_sheet_pin", id: "shpin_l", size_um: 2000 });
  assert.deepEqual(sheetPinEditCmd(pin, { name: "IN", shape: "input", sizeUm: 1270, bold: true, italic: true }), { op: "sch_edit", verb: "edit_sheet_pin", id: "shpin_l", bold: true, italic: true });
  const styled = { ...pin, size_um: 2000, bold: true };
  assert.equal(sheetPinEditCmd(styled, { name: "IN", shape: "input", sizeUm: 2000, bold: true, italic: false }), null);
  assert.deepEqual(sheetPinEditCmd(styled, { name: "IN", shape: "input", sizeUm: 1270, bold: false, italic: false }), { op: "sch_edit", verb: "edit_sheet_pin", id: "shpin_l", size_um: 1270, bold: false });
  // a pin set in a bigger text has a bigger box
  const big = { sheets: [{ ...sheet, pins: [{ id: "shpin_l", name: "IN", shape: "input", at: [100_000, 60_000], size_um: 2540 }, { id: "shpin_r", name: "OUT", shape: "output", at: [140_000, 70_000] }] }] } as unknown as Schematic;
  const [l, r] = sheetPinItems(big);
  assert.equal(l!.sizeUm, 2540);
  assert.equal(r!.sizeUm, 1270, "the default is 50 mil");
  const tall = (p: NonNullable<typeof l>) => { const b = sheetPinBox(p, measure); return b.maxY - b.minY; };
  assert.ok(tall(l!) > tall(r!) * 1.5, `${tall(l!)} against ${tall(r!)}`);
});
