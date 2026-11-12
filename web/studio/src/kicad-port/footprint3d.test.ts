import { test } from "node:test";
import assert from "node:assert/strict";
import type { FieldInfo, Pad, Part } from "../api/types";
import { fieldsDrawn3d, padHasCopper, padMaskMargin3d, padMaskSides, padPaste3d, padPasteMargin3d } from "./footprint3d";

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

// V7-1 (round 7): `PAD::GetSolderPasteMargin`: "Pads defined only on mask layers (and perhaps on other tech layers) use the shape defined by the pad settings only" --
// a pad on no copper layer gets no margin, the footprint's and the board's included.
test("a pad on the paste layer only gets no paste margin (PAD::GetSolderPasteMargin)", () => {
  const board = { paste_margin_nm: -50_000, paste_margin_ratio: 0 };
  assert.deepEqual(padPasteMargin3d(pad({ layers: ["F.Paste"] }), part([], { paste_margin: -80_000 }), board), [0, 0]);
  assert.deepEqual(padPaste3d(pad({ layers: ["F.Paste"] }), part([]), board), { w: 1_000_000, h: 600_000 }, "drawn the pad's size");
  // With copper it is grown as before; `*.Cu` counts.
  assert.deepEqual(padPasteMargin3d(pad({ layers: ["F.Cu", "F.Paste"] }), part([]), board), [-50_000, -50_000]);
  assert.equal(padHasCopper({ layers: ["*.Cu", "*.Mask"] }), true);
  assert.equal(padHasCopper({ layers: ["F.Paste", "F.Mask"] }), false);
});

// The solder mask's openings (task-u3-3d): `addPads` on F.Mask / B.Mask opens the mask where a pad is on it, grown by `PAD::GetSolderMaskExpansion`.
test("a pad opens the masks it is on, grown by its expansion (addPads, PAD::GetSolderMaskExpansion)", () => {
  assert.deepEqual(padMaskSides(pad({ layers: ["F.Cu", "F.Paste", "F.Mask"] }), "top"), ["top"]);
  assert.deepEqual(padMaskSides(pad({ layers: ["F.Cu", "F.Paste", "F.Mask"] }), "bottom"), ["bottom"], "F.Mask is the footprint's side");
  assert.deepEqual(padMaskSides(pad({ layers: ["*.Cu", "*.Mask"], th: true }), "top").sort(), ["bottom", "top"]);
  assert.deepEqual(padMaskSides(pad({ layers: ["F.Cu"] }), "top"), [], "a pad on no mask opens none");
  assert.deepEqual(padMaskSides(pad({ th: true }), "top").sort(), ["bottom", "top"], "a hole's kind: both");
  // The pad's own margin, else the board's; never less than minus half the smaller side; none for a pad with no copper.
  assert.equal(padMaskMargin3d(pad({ mask_margin: 50_000 }), part([], { mask_margin: 30_000 }), { expansion_nm: 20_000 }), 50_000);
  assert.equal(padMaskMargin3d(pad(), part([]), { expansion_nm: 20_000 }), 20_000);
  assert.equal(padMaskMargin3d(pad(), part([], { mask_margin: 30_000 }), { expansion_nm: 20_000 }), 30_000, "the footprint's own before the board's");
  assert.equal(padMaskMargin3d(pad({ mask_margin: -900_000 }), undefined, undefined), -300_000);
  assert.equal(padMaskMargin3d(pad({ layers: ["F.Mask"] }), undefined, { expansion_nm: 20_000 }), 0);
});
