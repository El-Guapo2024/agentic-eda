import { test } from "node:test";
import assert from "node:assert/strict";
import type { FieldInfo, Pad, Part } from "../api/types";
import { fieldsDrawn3d, padPaste3d } from "./footprint3d";

const field = (name: string, over: Partial<FieldInfo> = {}): FieldInfo => ({
  id: `R1:${name}`,
  name,
  text: name === "Reference" ? "R1" : name === "Value" ? "10k" : "x",
  x: 1_000_000,
  y: 2_000_000,
  angle: 90_000,
  w: 1_000_000,
  h: 1_000_000,
  thickness: 150_000,
  layer: "F.SilkS",
  visible: true,
  halign: 0,
  valign: 0,
  mirror: false,
  bold: false,
  italic: false,
  upright: false,
  knockout: false,
  lx: 0,
  ly: 0,
  langle: 0,
  custom: false,
  ...over,
});

const part = (fields: FieldInfo[], over: Partial<Part> = {}): Part => ({ ref: "R1", placed: true, side: "top", fields, ...over }) as unknown as Part;
const all = () => true;

// V6-4 (round 6): create_3Dgraphic_brd_items.cpp::addFootprintShapes draws a footprint's fields from the fields themselves: a Reference only with
// LAYER_FP_REFERENCES, a Value only with LAYER_FP_VALUES, every field only with LAYER_FP_TEXT, and only `field->IsVisible()` on the layer drawn.
test("the 3D view draws a footprint's visible fields, the Reference from its own field (addFootprintShapes)", () => {
  assert.deepEqual(fieldsDrawn3d(part([field("Reference", { visible: false }), field("Value", { layer: "F.Fab" })]), all).map((f) => f.name), ["Value"], "a hidden Reference is not drawn");
  const drawn = fieldsDrawn3d(part([field("Reference"), field("Value", { visible: false }), field("MPN")]), all);
  assert.deepEqual(drawn.map((f) => f.name), ["Reference", "MPN"]);
  // The field as it is: its position, size and angle.
  assert.deepEqual([drawn[0]!.x, drawn[0]!.y, drawn[0]!.h, drawn[0]!.angle, drawn[0]!.layer], [1_000_000, 2_000_000, 1_000_000, 90_000, "F.SilkS"]);
  // The rows that gate them.
  assert.deepEqual(fieldsDrawn3d(part([field("Reference"), field("Value")]), (row) => row !== "references").map((f) => f.name), ["Value"]);
  assert.deepEqual(fieldsDrawn3d(part([field("Reference"), field("Value")]), (row) => row !== "values").map((f) => f.name), ["Reference"]);
  assert.deepEqual(fieldsDrawn3d(part([field("Reference"), field("Value")]), (row) => row !== "footprint_text"), []);
  assert.deepEqual(fieldsDrawn3d(part([]), all), []);
});

const pad = (over: Partial<Pad> = {}): Pad => ({ num: "1", net: null, x: 0, y: 0, w: 1_000_000, h: 600_000, round: false, th: false, size: [1_000_000, 600_000], kind: "smd", ...over }) as Pad;

// V6-5 (round 6): BOARD_ADAPTER::addPads draws a pad on F.Paste / B.Paste only `if( pad->IsOnLayer( aLayerId ) )`, grown by
// `PAD::GetSolderPasteMargin`: the pad's margin, else the footprint's, else the board's; the ratio likewise; `margin + KiROUND( size * ratio )` per axis,
// no smaller than half the pad.
test("solder paste follows the pad's paste layer and margin (addPads, PAD::GetSolderPasteMargin)", () => {
  assert.deepEqual(padPaste3d(pad(), part([]), undefined), { w: 1_000_000, h: 600_000 });
  assert.deepEqual(padPaste3d(pad({ paste_margin: -100_000 }), part([]), undefined), { w: 800_000, h: 400_000 }, "0.2 mm smaller");
  // The footprint's, then the board's.
  assert.deepEqual(padPaste3d(pad(), part([], { paste_margin: -50_000 }), { paste_margin_nm: -20_000, paste_margin_ratio: 0 }), { w: 900_000, h: 500_000 });
  assert.deepEqual(padPaste3d(pad(), part([]), { paste_margin_nm: -20_000, paste_margin_ratio: -0.1 }), { w: 1_000_000 - 2 * 120_000, h: 600_000 - 2 * 80_000 });
  // The pad's own ratio wins over the board's, and a margin is never more than half the pad.
  assert.deepEqual(padPaste3d(pad({ paste_ratio: 0 }), part([]), { paste_margin_nm: 0, paste_margin_ratio: -0.1 }), { w: 1_000_000, h: 600_000 });
  assert.deepEqual(padPaste3d(pad({ paste_margin: -400_000 }), part([]), undefined), { w: 200_000, h: 0 });
  // A turned pad: the margin of each axis goes to the side it lies along on the board.
  assert.deepEqual(padPaste3d(pad({ w: 600_000, h: 1_000_000, paste_ratio: -0.1 }), part([]), undefined), { w: 600_000 - 2 * 60_000, h: 1_000_000 - 2 * 100_000 });
  // Not on the paste layer: no paste (a fiducial, a connector pad); a through-hole pad has none unless its layers say so.
  assert.equal(padPaste3d(pad({ layers: ["F.Cu", "F.Mask"] }), part([]), undefined), null);
  assert.deepEqual(padPaste3d(pad({ layers: ["F.Cu", "F.Paste", "F.Mask"] }), part([]), undefined), { w: 1_000_000, h: 600_000 });
  assert.equal(padPaste3d(pad({ th: true, kind: "through_hole" }), part([]), undefined), null);
});
