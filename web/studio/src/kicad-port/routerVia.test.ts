import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, NetClass } from "../api/types";
import { chosenTrackWidthOf } from "./routerSizes";
import { chosenViaOf, chosenViaSize, DEFAULT_CUSTOM_TRACK_VIA, DEFAULT_MICROVIA, pickableLayers, planViaCommand, validateViaParameters, VIA_COMMANDS, VIA_NEEDS_TWO_LAYERS, viaSizeFor, type ViaCommand } from "./routerVia";

const CU4 = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
const blind: ViaCommand = { type: "blind", selectLayer: false };
const micro: ViaCommand = { type: "micro", selectLayer: false };
const pick = (type: ViaCommand["type"]): ViaCommand => ({ type, selectLayer: true });

test("the router's via actions carry KiCad's VIA_ACTION_FLAGS", () => {
  assert.deepEqual(VIA_COMMANDS["pcbnew.InteractiveRouter.PlaceBlindVia"], blind);
  assert.deepEqual(VIA_COMMANDS["pcbnew.InteractiveRouter.PlaceMicroVia"], micro);
  assert.deepEqual(VIA_COMMANDS["pcbnew.InteractiveRouter.SelLayerAndPlaceVia"], pick("through"));
  assert.deepEqual(VIA_COMMANDS["pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia"], pick("blind"));
  assert.deepEqual(VIA_COMMANDS["pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia"], pick("micro"));
});

test("a blind via on a layer of the pair spans the pair, and the route goes on on the pair's other layer", () => {
  const pair = { a: "F.Cu", b: "In1.Cu" };
  assert.deepEqual(planViaCommand({ cmd: blind, currentLayer: "F.Cu", copper: CU4, layerPair: pair }), { ok: true, viaType: "blind", span: ["F.Cu", "In1.Cu"], pair: ["F.Cu", "In1.Cu"], toLayer: "In1.Cu" });
  // from the bottom layer of the pair: `currentLayer = pairTop; targetLayer = pairBottom`, and `PairedLayer( In1 )` is F.Cu
  assert.deepEqual(planViaCommand({ cmd: blind, currentLayer: "In1.Cu", copper: CU4, layerPair: pair }), { ok: true, viaType: "blind", span: ["F.Cu", "In1.Cu"], pair: ["F.Cu", "In1.Cu"], toLayer: "F.Cu" });
});

test("off the pair a blind via goes to the pair's top layer", () => {
  const plan = planViaCommand({ cmd: blind, currentLayer: "B.Cu", copper: CU4, layerPair: { a: "In1.Cu", b: "In2.Cu" } });
  assert.deepEqual(plan, { ok: true, viaType: "blind", span: ["In1.Cu", "B.Cu"], pair: ["In1.Cu", "B.Cu"], toLayer: "In1.Cu" });
});

test("a pair of one layer is a broken via: KiCad's message", () => {
  assert.deepEqual(planViaCommand({ cmd: micro, currentLayer: "F.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "F.Cu" } }), { ok: false, message: VIA_NEEDS_TWO_LAYERS });
  assert.equal(VIA_NEEDS_TWO_LAYERS, "Via needs 2 different layers.");
});

test("the implicit blind via on the F.Cu/B.Cu pair stays a blind via (the through conversion comes before the implicit target, as in KiCad)", () => {
  const plan = planViaCommand({ cmd: blind, currentLayer: "F.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "B.Cu" } });
  assert.deepEqual(plan, { ok: true, viaType: "blind", span: ["F.Cu", "B.Cu"], pair: ["F.Cu", "B.Cu"], toLayer: "B.Cu" });
});

test("a picked layer: a blind or micro via from one outer layer to the other is a through via", () => {
  assert.deepEqual(planViaCommand({ cmd: pick("micro"), currentLayer: "B.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "B.Cu" }, picked: "F.Cu" }), { ok: true, viaType: "through", span: ["F.Cu", "B.Cu"], pair: ["F.Cu", "B.Cu"], toLayer: "F.Cu" });
  assert.deepEqual(planViaCommand({ cmd: pick("micro"), currentLayer: "F.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "B.Cu" }, picked: "In1.Cu" }), { ok: true, viaType: "micro", span: ["F.Cu", "In1.Cu"], pair: ["F.Cu", "In1.Cu"], toLayer: "In1.Cu" });
  assert.deepEqual(planViaCommand({ cmd: pick("blind"), currentLayer: "In2.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "B.Cu" }, picked: "In1.Cu" }), { ok: true, viaType: "blind", span: ["In1.Cu", "In2.Cu"], pair: ["In1.Cu", "In2.Cu"], toLayer: "In1.Cu" });
});

test("a picked layer for a through via: the via spans the board, the route goes on on the picked layer", () => {
  assert.deepEqual(planViaCommand({ cmd: pick("through"), currentLayer: "F.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "B.Cu" }, picked: "In2.Cu" }), { ok: true, viaType: "through", span: ["F.Cu", "B.Cu"], pair: ["F.Cu", "In2.Cu"], toLayer: "In2.Cu" });
  // the current layer is allowed for a through via: it stays on it (`PairedLayer` of a pair of one layer)
  assert.deepEqual(planViaCommand({ cmd: pick("through"), currentLayer: "In1.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "B.Cu" }, picked: "In1.Cu" }), { ok: true, viaType: "through", span: ["F.Cu", "B.Cu"], pair: ["In1.Cu", "In1.Cu"], toLayer: "In1.Cu" });
});

test("nothing without a route, a cancelled picker, or a blind via picked to its own layer", () => {
  assert.deepEqual(planViaCommand({ cmd: blind, currentLayer: null, copper: CU4, layerPair: { a: "F.Cu", b: "In1.Cu" } }), { ok: false, message: null });
  assert.deepEqual(planViaCommand({ cmd: pick("blind"), currentLayer: "F.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "B.Cu" }, picked: null }), { ok: false, message: null });
  assert.deepEqual(planViaCommand({ cmd: pick("blind"), currentLayer: "F.Cu", copper: CU4, layerPair: { a: "F.Cu", b: "B.Cu" }, picked: "F.Cu" }), { ok: false, message: null });
});

test("the picker offers the copper layers, less the current one for a via that is not a through via", () => {
  assert.deepEqual(pickableLayers(pick("through"), CU4, "In1.Cu"), CU4);
  assert.deepEqual(pickableLayers(pick("blind"), CU4, "In1.Cu"), ["F.Cu", "In2.Cu", "B.Cu"]);
  assert.deepEqual(pickableLayers(pick("micro"), CU4, "F.Cu"), ["In1.Cu", "In2.Cu", "B.Cu"]);
});

const board = (classes: NetClass[], defaultClass?: NetClass): BoardState =>
  ({ layers: CU4, board_rules: { track_width: 250_000, via_diameter: 600_000, via_drill: 300_000, clearance: 200_000, net_classes: classes, default_class: defaultClass } }) as unknown as BoardState;

test("a microvia takes its net class's microvia size, else KiCad's 0.3 / 0.1 mm", () => {
  const b = board([{ name: "hdi", nets: ["USB*"], microvia_diameter: 250_000, microvia_drill: 80_000, priority: 0 }]);
  assert.deepEqual(viaSizeFor(b, "USB_DP", "micro", { diameter: 800_000, drill: 400_000 }), { diameter: 250_000, drill: 80_000 }, "even with a via size chosen");
  assert.deepEqual(viaSizeFor(b, "GND", "micro", null), DEFAULT_MICROVIA);
  assert.deepEqual(viaSizeFor(board([], { name: "Default", nets: [], microvia_diameter: 200_000, microvia_drill: 100_000, priority: 0 }), "GND", "micro", null), { diameter: 200_000, drill: 100_000 });
});

test("a blind or through via takes the chosen via size, else the net class's", () => {
  const b = board([{ name: "pwr", nets: ["VBAT"], via_diameter: 800_000, via_drill: 400_000, priority: 0 }]);
  assert.deepEqual(viaSizeFor(b, "VBAT", "blind", null), { diameter: 800_000, drill: 400_000 });
  assert.deepEqual(viaSizeFor(b, "GND", "blind", null), { diameter: 600_000, drill: 300_000 });
  assert.deepEqual(viaSizeFor(b, "VBAT", "through", { diameter: 500_000, drill: 250_000 }), { diameter: 500_000, drill: 250_000 });
});

test("PCB_VIA::ValidateViaParameters' messages, word for word", () => {
  assert.equal(validateViaParameters(0, 300_000), "Via diameter is too small.");
  assert.equal(validateViaParameters(600_000, 0), "Via drill is too small.");
  assert.equal(validateViaParameters(600_000, null), "No via hole size defined.");
  assert.equal(validateViaParameters(null, 300_000), "No via diameter defined.");
  assert.equal(validateViaParameters(300_000, 300_000), "Via hole size must be smaller than via diameter");
  assert.equal(validateViaParameters(600_000, 300_000), null);
});

// r3 R3-3 / r4 R4-8: `ImportSizes`' `UseNetClassTrack()` -- with no width chosen, or the list's first entry (index 0, the net class's), a
// route sends no width and takes the start net's class width from the server; a chosen width or a custom size is sent. The choice is the
// list INDEX, as `m_trackWidthIndex == 0` upstream, not the width: a preset that happens to equal the Default class's width is a user choice.
test("a route takes the start net's class width unless a width is chosen", () => {
  // list = [netclass 250_000, preset 250_000, preset 400_000] (r5 R5-9: the presets are read at route time, kicad-port/routerSizes.ts)
  const presets = [250_000, 400_000];
  assert.equal(chosenTrackWidthOf(presets, null, false, null), undefined, "nothing chosen: the net class's");
  assert.equal(chosenTrackWidthOf(presets, 0, false, null), undefined, "index 0 is the net class's");
  assert.equal(chosenTrackWidthOf(presets, 2, false, null), 400_000, "a width of the list");
  // r4 R4-8: the same width as the Default class's, but at index > 0, is the user's choice (ImportSizes: source "user choice").
  assert.equal(chosenTrackWidthOf(presets, 1, false, null), 250_000, "a preset equal to the Default class's width is still a chosen width");
  assert.equal(chosenTrackWidthOf(presets, 3, false, null), undefined, "past the list's end is the net class's (GetCurrentTrackWidth's range check)");
  assert.equal(chosenTrackWidthOf(presets, 1, true, 250_000), 250_000, "a custom size is a size");
});

// r3 R3-4 / r4 R4-8: `handleLayerSwitch`' `UseNetClassVia() || viaType == MICROVIA` -- the server asks the rules for the via's size then.
// The choice is the list INDEX too: a preset equal to the net class's size at index > 0 is chosen.
test("a via takes the net class's size from the server unless a size is chosen", () => {
  const base = { diameter: 600_000, drill: 300_000 };
  // list = [netclass 600/300, preset 600/300, preset 800/400]
  assert.equal(chosenViaSize("through", null, null, 3, false), null);
  assert.equal(chosenViaSize("through", base, 0, 3, false), null, "index 0 is the net class's");
  assert.deepEqual(chosenViaSize("blind", { diameter: 800_000, drill: 400_000 }, 2, 3, false), { diameter: 800_000, drill: 400_000 });
  assert.deepEqual(chosenViaSize("through", base, 1, 3, false), base, "a preset equal to the net class's size is still a chosen size");
  assert.equal(chosenViaSize("through", base, 3, 3, false), null, "past the list's end is the net class's");
  assert.equal(chosenViaSize("micro", { diameter: 800_000, drill: 400_000 }, 2, 3, false), null, "a microvia's is always the net class's");
  assert.deepEqual(chosenViaSize("through", base, 0, 3, true), base, "a custom size is a size");
});

test("R6-R1: V while routing sizes the via as the other via commands (handleLayerSwitch): none at index 0, the list's entry, the custom size", () => {
  const presets = [{ diameter: 700_000, drill: 350_000 }, { diameter: 800_000, drill: 400_000 }];
  const choice = { presets, index: 0, currentPreset: null, useCustomTrackVia: false, custom: DEFAULT_CUSTOM_TRACK_VIA };
  assert.equal(chosenViaOf("through", choice), null, "the net class's: the server asks the rules for the route's net");
  assert.deepEqual(chosenViaOf("through", { ...choice, index: 2 }), presets[1]);
  assert.deepEqual(chosenViaOf("through", { ...choice, index: 2, useCustomTrackVia: true }), { diameter: DEFAULT_CUSTOM_TRACK_VIA.viaDiameterNm, drill: DEFAULT_CUSTOM_TRACK_VIA.viaDrillNm });
  assert.equal(chosenViaOf("micro", { ...choice, index: 2 }), null, "a microvia's is the rules'");
});
