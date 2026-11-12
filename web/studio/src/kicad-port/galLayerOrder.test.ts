// `GAL_LAYER_ORDER` and `PCB_DRAW_PANEL_GAL::SetTopLayer` (KiCad 25ec2335 pcbnew/pcb_draw_panel_gal.cpp): the order the board painter draws its layers in.
import { test } from "node:test";
import assert from "node:assert/strict";
import { galDrawOrder, galLayerOrderTopFirst } from "./galLayerOrder";

const before = (order: string[], a: string, b: string) => order.indexOf(a) >= 0 && order.indexOf(b) >= 0 && order.indexOf(a) < order.indexOf(b);

test("the list: the overlays first, then the drawing layers, the via and pad holes, F side, inner layers, B side, images, the sheet last", () => {
  const top = galLayerOrderTopFirst();
  assert.equal(top[0], "LAYER_UI");
  assert.equal(top[top.length - 1], "LAYER_DRAWINGSHEET");
  assert.ok(before(top, "LAYER_VIA_HOLES", "LAYER_PAD_PLATEDHOLES") && before(top, "LAYER_PAD_PLATEDHOLES", "pad:F.Cu"));
  assert.ok(before(top, "pad:F.Cu", "F.Cu") && before(top, "F.Cu", "zone:F.Cu"), "a layer's pads over its tracks over its zones");
  assert.ok(before(top, "F.Fab", "In1.Cu") && before(top, "In1.Cu", "In2.Cu") && before(top, "In30.Cu", "B.Cu"));
  assert.ok(before(top, "B.Fab", "bitmap:Dwgs.User"));
});

test("SetTopLayer( F.Cu ): the front layers over the inner and back ones -- F.Cu, the active one, over the front technical layers -- the holes over every copper layer", () => {
  const order = galDrawOrder("F.Cu");
  // the images of the other layers are put behind everything, the sheet too (`SetLayerOrder( layer, i - TOP_LAYER_MODIFIER )`), the active layer's over the sheet
  assert.ok(order[0]!.startsWith("bitmap:") && before(order, "bitmap:B.Fab", "LAYER_DRAWINGSHEET") && before(order, "LAYER_BOARD_OUTLINE_AREA", "bitmap:F.Cu"));
  assert.ok(before(order, "B.Cu", "In1.Cu") && before(order, "In1.Cu", "F.Cu"));
  assert.ok(before(order, "zone:F.Cu", "F.Cu") && before(order, "F.Cu", "pad:F.Cu"));
  assert.ok(before(order, "pad:F.Cu", "LAYER_PAD_PLATEDHOLES") && before(order, "LAYER_PAD_HOLEWALLS", "LAYER_PAD_PLATEDHOLES") && before(order, "LAYER_VIA_HOLEWALLS", "LAYER_VIA_HOLES"));
  assert.ok(before(order, "F.SilkS", "F.Cu") && before(order, "B.Cu", "F.SilkS"), "the front silkscreen raised with the side, under F.Cu (earlier in the list)");
  assert.ok(before(order, "B.SilkS", "B.Cu"), "the back silkscreen under the back copper");
  assert.ok(before(order, "bitmap:F.Cu", "B.Fab") && before(order, "bitmap:Dwgs.User", "bitmap:F.Cu"), "images under every layer, the active one's over the others");
  assert.ok(before(order, "LAYER_DRC_ERROR", "LAYER_SELECT_OVERLAY") && before(order, "LAYER_SELECT_OVERLAY", "LAYER_GP_OVERLAY"));
});

test("SetTopLayer( B.Cu ): the back layers rise over the front ones, B.Cu over its silkscreen, its pads over its tracks, still under the holes", () => {
  const order = galDrawOrder("B.Cu");
  assert.ok(before(order, "F.Cu", "B.Cu") && before(order, "pad:F.Cu", "pad:B.Cu"));
  assert.ok(before(order, "F.SilkS", "B.SilkS") && before(order, "B.SilkS", "B.Cu") && before(order, "B.Cu", "pad:B.Cu"));
  assert.ok(before(order, "pad:B.Cu", "LAYER_PAD_PLATEDHOLES"));
  assert.ok(before(order, "LAYER_PAD_FR_NETNAMES", "LAYER_PAD_BK_NETNAMES"));
});

test("SetTopLayer( In2.Cu ): only that inner layer, its zones, pads and vias rise; the sides keep their order", () => {
  const order = galDrawOrder("In2.Cu");
  assert.ok(before(order, "F.Cu", "In2.Cu") && before(order, "F.SilkS", "In2.Cu") && before(order, "In1.Cu", "F.Cu"));
  assert.ok(before(order, "In2.Cu", "pad:In2.Cu") && before(order, "pad:F.Cu", "pad:In2.Cu") && before(order, "pad:In2.Cu", "LAYER_PAD_PLATEDHOLES"));
});

test("SetTopLayer( F.SilkS ): an active technical layer goes just under the marker shadows, over the vias and pads, its zones under it", () => {
  const order = galDrawOrder("F.SilkS");
  assert.ok(before(order, "LAYER_VIA_HOLES", "F.SilkS") && before(order, "LAYER_PAD_PLATEDHOLES", "F.SilkS"));
  assert.ok(before(order, "zone:F.SilkS", "F.SilkS") && before(order, "F.SilkS", "LAYER_MARKER_SHADOWS"));
  assert.ok(before(order, "LAYER_PAD_NETNAMES", "zone:F.SilkS"), "the pad net names under it");
  assert.ok(before(order, "F.Mask", "F.Cu") && before(order, "B.Cu", "F.Cu"), "the other front layers raised with the side, in the list's order");
});

test("no active layer stands for F.Cu (KiCad's frame always has one)", () => {
  assert.deepEqual(galDrawOrder(null), galDrawOrder("F.Cu"));
});
