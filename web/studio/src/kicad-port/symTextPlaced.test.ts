import { test } from "node:test";
import assert from "node:assert/strict";
import { symbolTransformMatrix } from "../components/schematic/transform";
import { itemOrientationOf } from "./symTextBoxPlaced";
import { orientedDeviceText, type HAlign, type VAlign } from "./symTextPlaced";

const placed = (rot: number, mirror: "x" | "y" | null, vertical: boolean, h: HAlign, v: VAlign) => orientedDeviceText(vertical, h, v, itemOrientationOf(symbolTransformMatrix(rot, mirror)));

test("a symbol's text turned with the symbol: each quarter turn toggles its direction, and a vertical text that turns flips its horizontal justification (`SCH_TEXT::Rotate90( false )`)", () => {
  assert.deepEqual(placed(0, null, false, "left", "top"), { vertical: false, h: "left", v: "top" }, "unturned: as the library has it");
  assert.deepEqual(placed(90, null, false, "left", "top"), { vertical: true, h: "left", v: "top" }, "a horizontal text runs up the sheet, read from its left still");
  assert.deepEqual(placed(180, null, false, "left", "top"), { vertical: false, h: "right", v: "top" }, "a half turn: up, then down again with the justification flipped");
  assert.deepEqual(placed(270, null, false, "left", "top"), { vertical: true, h: "right", v: "top" });
  assert.deepEqual(placed(90, null, true, "left", "center"), { vertical: false, h: "right", v: "center" }, "a vertical text lies down, its left the right now");
});

test("a symbol's mirrored text keeps reading forwards: the justification along the mirror flips (`SCH_TEXT::MirrorHorizontally` / `MirrorVertically` on `LAYER_DEVICE`)", () => {
  // left to right (KiCad's mirror Y): a horizontal text's left and right, a vertical one's top and bottom
  assert.deepEqual(placed(0, "y", false, "left", "top"), { vertical: false, h: "right", v: "top" });
  assert.deepEqual(placed(0, "y", true, "left", "top"), { vertical: true, h: "left", v: "bottom" });
  // top to bottom (KiCad's mirror X): a horizontal text's top and bottom, a vertical one's left and right
  assert.deepEqual(placed(0, "x", false, "left", "top"), { vertical: false, h: "left", v: "bottom" });
  assert.deepEqual(placed(0, "x", true, "left", "top"), { vertical: true, h: "right", v: "top" });
  // centred stays centred whatever the orientation
  for (const rot of [0, 90, 180, 270]) for (const m of [null, "x", "y"] as const) assert.deepEqual(placed(rot, m, false, "center", "center").h, "center");
});
