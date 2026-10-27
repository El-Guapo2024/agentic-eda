import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { compilePcbExpr, evaluateFor, itemLayers, itemsMatching, matchDpSuffix, numberWithUnit, pcbExprEnv, type PcbExprEnv } from "./pcbExpr";
import { pcbItemOf, pcbItemsOf, type PcbItem } from "./pcbProperties";
import { pcbExprBoard } from "./pcbExprFixture";

const board: BoardState = pcbExprBoard();
const env: PcbExprEnv = pcbExprEnv(board);
const item = (id: string): PcbItem => {
  const i = pcbItemOf(board, id);
  assert.ok(i, `the fixture has ${id}`);
  return i;
};

/** The truth of `text` for A (and B) on `layer`. */
const holds = (text: string, a: string | null, b: string | null = null, layer = "F.Cu"): boolean => {
  const r = evaluateFor(text, env, a ? item(a) : null, b ? item(b) : null, layer);
  assert.equal(r.compiled.ok, true, `${text}: ${r.compiled.errors.map((e) => e.message).join("; ")}`);
  return r.value.asDouble() !== 0;
};

const ids = (text: string): string[] => {
  const c = compilePcbExpr(text, env);
  assert.equal(c.ok, true, text);
  const all = pcbItemsOf(board, [
    ...board.routing!.tracks.map((t) => t.id),
    ...board.routing!.vias.map((v) => v.id),
    ...board.routing!.zones.map((z) => z.id),
    ...board.parts.filter((p) => p.placed).map((p) => p.ref),
    "U1.1",
    "U1.2",
    "U1.3",
    "U1.4",
    "R1.1",
    "R2.1",
    ...board.drawings!.shapes.map((s) => s.id),
    ...board.drawings!.texts.map((t) => t.id),
  ]);
  return itemsMatching(c, all, env);
};

// ---------------------------------------------------------------------------------------------------------------------------------- properties

test("A.Type is the class by KiCad's names, and compares without case", () => {
  assert.equal(holds("A.Type == 'Via'", "via_a"), true);
  assert.equal(holds("A.Type == 'via'", "via_a"), true);
  assert.equal(holds("A.Type == 'Track'", "trk_arc"), true, "an arc is a Track");
  assert.equal(holds("A.Type == 'Pad'", "U1.1"), true);
  assert.equal(holds("A.Type == 'Footprint'", "U1"), true);
  assert.equal(holds("A.Type == 'Graphic'", "shp_seg"), true);
  assert.equal(holds("A.Type == 'Text'", "txt_a"), true);
  assert.equal(holds("A.Type == 'Zone'", "zone_ra"), true, "a rule area is a Zone");
  assert.equal(holds("A.Type != 'Pad'", "U1.1"), false);
  assert.equal(holds("A.Type == '*a*'", "trk_a"), true, "a pattern on the right");
});

test("lengths are in nanometres: a width of 200 um is 0.2mm", () => {
  assert.equal(holds("A.Width == 0.2mm", "trk_a"), true);
  assert.equal(holds("A.Width > 0.25mm", "trk_a"), false);
  assert.equal(holds("A.Width > 0.25mm", "trk_b"), true);
  assert.equal(holds("A.Width < 10mil", "trk_a"), true, "10 mil is 0.254 mm");
  assert.equal(holds("A.Width >= 200000", "trk_a"), true, "a number with no unit is nanometres too");
  assert.equal(holds("A.Diameter == 0.6mm && A.Hole == 0.3mm", "via_a"), true);
  assert.equal(holds("A.Position_X < 20mm", "via_a"), false);
  assert.equal(holds("A.Position_X == 20mm && A.Position_Y == 50mm", "via_a"), true);
  assert.equal(holds("A.Orientation == -90deg", "U1"), true, "the footprint's orientation, counter-clockwise, in (-180, 180] as FOOTPRINT::GetOrientationDegrees gives it");
  assert.equal(holds("A.Width + 0.1mm == 0.3mm", "trk_a"), true);
});

test("a property is found without regard to case and with _ for a blank; one the class lacks is undefined, false both ways", () => {
  assert.equal(holds("A.width == 0.2mm", "trk_a"), true);
  assert.equal(holds("A.Pad_Type == 'SMD'", "U1.1"), true);
  assert.equal(holds("A.Pad_Type == 'Through-hole'", "U1.2"), true);
  assert.equal(holds("A.Pad_Shape == 'Oval'", "U1.3"), true);
  assert.equal(holds("A.Pad_Number == '3'", "U1.3"), true, "a plain number stays text");
  assert.equal(holds("A.Via_Type == 'Through'", "via_a"), true);
  assert.equal(holds("A.Via_Type == 'Blind'", "via_b"), true);
  assert.equal(holds("A.Via_Type == 'Buried'", "via_c"), true);
  assert.equal(holds("A.Via_Type == 'Through'", "trk_a"), false, "a track has no via type");
  assert.equal(holds("A.Via_Type != 'Through'", "trk_a"), false, "and it is not different from one either");
  assert.equal(holds("A.Width == 0.2mm", "via_a"), false);
});

test("a pad reads a footprint's property through its footprint: A.Reference on a pad is the footprint's", () => {
  assert.equal(holds("A.Reference == 'U1'", "U1.2"), true);
  assert.equal(holds("A.Reference == 'U*'", "U1.2"), true);
  assert.equal(holds("A.Reference == 'R*'", "U1.2"), false);
  // `FOOTPRINT_DESC` has no Value property (r2-pcb-edit A6): the field is read with getField
  assert.equal(holds("A.getField('Value') == 'MCU'", "U1"), true);
  assert.equal(holds("A.Library_Link == 'Package_QFP:LQFP-48'", "U1"), true);
});

test("a flag is 1 or 0, a choice its label", () => {
  assert.equal(holds("A.Locked", "trk_b"), true);
  assert.equal(holds("A.Locked", "trk_a"), false);
  assert.equal(holds("!A.Locked", "trk_a"), true);
  assert.equal(holds("A.Locked == 1", "trk_b"), true);
  assert.equal(holds("A.Layer == 'B.Cu'", "R1"), true, "a footprint on the bottom is on B.Cu");
  assert.equal(holds("A.Layer == 'F.Cu'", "R2"), true);
});

test("a text property that is a length with a unit reads as one", () => {
  assert.equal(numberWithUnit("0.2mm"), 200_000);
  assert.equal(numberWithUnit("5 mil"), 127_000);
  assert.equal(numberWithUnit("1in"), 25_400_000);
  assert.equal(numberWithUnit("90ra"), 90 * (180 / Math.PI));
  assert.equal(numberWithUnit("12"), null, "a bare number is not one");
  assert.equal(numberWithUnit("abc"), null);
});

// ------------------------------------------------------------------------------------------------------------------------------------ nets

test("net names, net classes, a composite class", () => {
  assert.equal(holds("A.NetName == 'GND'", "trk_a"), true);
  assert.equal(holds("A.NetName == 'G*'", "trk_a"), true);
  assert.equal(holds("A.NetName == 'GND'", "U1"), false, "a footprint has no net");
  assert.equal(holds("A.Net == B.Net", "trk_a", "trk_c"), true);
  assert.equal(holds("A.Net != B.Net", "trk_a", "trk_b"), true);
  assert.equal(holds("A.NetName == B.NetName", "trk_a", "via_a"), true);
  assert.equal(holds("A.NetClass == 'HV'", "trk_b"), true, "VCC is in HV (and Default)");
  assert.equal(holds("A.NetClass == 'Default'", "trk_b"), true, "HV sets only a clearance: Default gives the rest, and a composite class is each of its constituents");
  assert.equal(holds("A.NetClass == 'HV,Default'", "trk_b"), true, "and the whole name");
  assert.equal(holds("A.NetClass == 'Default'", "trk_a"), true);
  assert.equal(holds("A.NetClass != 'HV'", "trk_a"), true);
  assert.equal(holds("A.NetClass != 'HV'", "trk_b"), false);
  assert.equal(holds("A.NetClass == B.NetClass", "trk_p", "trk_n"), true);
  assert.equal(holds("A.NetClass == B.NetClass", "trk_a", "trk_b"), false);
  assert.equal(holds("A.hasNetclass('HV')", "trk_b"), true);
  assert.equal(holds("A.hasNetclass('HV')", "trk_a"), false);
  assert.equal(holds("A.hasNetclass('Default')", "trk_a"), true);
  assert.equal(holds("A.hasExactNetclass('Default')", "trk_a"), true);
  assert.equal(holds("A.hasExactNetclass('HV')", "trk_b"), false, "HV,Default is not exactly HV");
  assert.equal(holds("A.hasNetclass('Fast')", "U1"), false, "a footprint is not connected");
  assert.equal(holds("A.Net_Class == 'Default'", "trk_a"), true, "the alias of the hidden property");
});

test("a differential pair: inDiffPair and isCoupledDiffPair", () => {
  assert.deepEqual(matchDpSuffix("USB_P"), { polarity: 1, base: "USB_", complement: "USB_N" });
  assert.deepEqual(matchDpSuffix("TX-"), { polarity: -1, base: "TX", complement: "TX+" });
  assert.deepEqual(matchDpSuffix("CLK_N2"), { polarity: -1, base: "CLK_", complement: "CLK_P2" });
  assert.equal(matchDpSuffix("GND").polarity, 0);
  assert.equal(holds("A.inDiffPair('USB*')", "trk_p"), true);
  assert.equal(holds("A.inDiffPair('USB')", "trk_p"), true, "the base name with its trailing _ left off");
  assert.equal(holds("A.inDiffPair('SPI*')", "trk_p"), false);
  assert.equal(holds("A.inDiffPair('*')", "trk_a"), false, "GND has no partner");
  assert.equal(holds("AB.isCoupledDiffPair()", "trk_p", "trk_n"), true);
  assert.equal(holds("AB.isCoupledDiffPair()", "trk_p", "trk_a"), false);
  assert.equal(holds("A.hasNetclass('Fast') && A.inDiffPair('*')", "trk_n"), true);
});

// ------------------------------------------------------------------------------------------------------------------------------- layers, groups

test("a layer matches the names of the layers a pattern says, canonical or the board's", () => {
  assert.equal(holds("A.Layer == 'F.Cu'", "trk_a"), true);
  assert.equal(holds("A.Layer == '*.Cu'", "trk_a"), true);
  assert.equal(holds("A.Layer == 'B.*'", "trk_a"), false);
  assert.equal(holds("A.Layer == 'F.Silkscreen'", "shp_seg"), true, "the board's name for F.SilkS");
  assert.equal(holds("A.Layer == 'F.SilkS'", "shp_seg"), true, "and the canonical one");
  assert.equal(holds("A.Layer != 'F.SilkS'", "shp_seg"), false);
  assert.equal(holds("A.Layer == 'F.Cu'", "trk_a", null, "B.Cu"), true, "A.Layer is the item's own, not the layer being tried");
  assert.equal(holds("L == 'B.Cu'", "trk_a", null, "B.Cu"), true, "L is the layer being tried");
  assert.equal(holds("L == 'B.Cu'", "trk_a", null, "F.Cu"), false);
  assert.equal(holds("A.existsOnLayer('F.Cu')", "trk_a"), true);
  assert.equal(holds("A.existsOnLayer('B.Cu')", "trk_a"), false);
  assert.equal(holds("A.existsOnLayer('*.Cu')", "via_a"), true);
  assert.equal(holds("A.existsOnLayer('In1.Cu')", "via_a"), true, "a through via is on every copper layer");
  assert.equal(holds("A.existsOnLayer('In2.Cu')", "via_b"), false, "a blind via from F.Cu to In1.Cu is not");
  assert.equal(holds("A.existsOnLayer('*.Mask')", "U1.2"), true);
  assert.deepEqual(itemLayers(item("via_c"), env), ["In1.Cu", "In2.Cu"]);
  assert.deepEqual(itemLayers(item("U1.1"), env), ["F.Cu", "F.Paste", "F.Mask"]);
  assert.deepEqual(itemLayers(item("R1.1"), env), ["B.Cu", "B.Paste", "B.Mask"]);
  assert.deepEqual(itemLayers(item("U1.2"), env), ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu", "F.Mask", "B.Mask"]);
});

test("vias: isPlated, isBlindVia, isBuriedVia, isBlindBuriedVia, isMicroVia", () => {
  assert.equal(holds("A.isPlated()", "via_a"), true);
  assert.equal(holds("A.isPlated()", "U1.2"), true);
  assert.equal(holds("A.isPlated()", "U1.1"), false);
  assert.equal(holds("A.isPlated()", "trk_a"), false);
  assert.equal(holds("A.isBlindVia()", "via_b"), true);
  assert.equal(holds("A.isBlindVia()", "via_a"), false);
  assert.equal(holds("A.isBuriedVia()", "via_c"), true);
  assert.equal(holds("A.isBlindBuriedVia()", "via_b"), true);
  assert.equal(holds("A.isBlindBuriedVia()", "via_a"), false);
  assert.equal(holds("A.isMicroVia()", "via_b"), false);
});

test("groups and footprints: memberOf, memberOfFootprint, getField", () => {
  assert.equal(holds("A.memberOfGroup('block')", "trk_a"), true);
  assert.equal(holds("A.memberOf('bl*')", "U1"), true);
  assert.equal(holds("A.memberOfGroup('block')", "U1.1"), true, "a pad is in its footprint's group");
  assert.equal(holds("A.memberOfGroup('block')", "trk_b"), false);
  assert.equal(holds("A.memberOfFootprint('U1')", "U1.1"), true);
  assert.equal(holds("A.memberOfFootprint('R*')", "U1.1"), false);
  assert.equal(holds("A.memberOfFootprint('Package_QFP:*')", "U1.1"), true, "a library id when the selector has a colon");
  assert.equal(holds("A.memberOfFootprint('U1')", "U1"), false, "a footprint is not a member of itself");
  assert.equal(holds("A.getField('Supplier') == 'ACME'", "U1"), true);
  assert.equal(holds("A.getField('Supplier') == 'ACME'", "R2"), false);
  assert.equal(holds("A.getField('Nope') == ''", "U1"), true);
  assert.equal(holds("A.getField('Supplier') == 'ACME'", "trk_a"), false, "only a footprint has fields");
});

// ----------------------------------------------------------------------------------------------------------------------------------- compiling

test("what the compiler tells: unknown properties, functions and items are reported and the text still compiles; a syntax error does not", () => {
  const told = (text: string) => compilePcbExpr(text, env).errors.map((e) => e.message);
  assert.deepEqual(told("A.Nope == 'x'"), ["Unrecognized property 'Nope'"]);
  assert.deepEqual(told("A.Nope == 1"), ["Unrecognized property 'Nope'", "Missing units for '1'| (must be mm, in, mil, deg, fs, or ps)"]);
  assert.deepEqual(told("C.Width > 1mm"), ["Unrecognized item 'C'"]);
  assert.deepEqual(told("A.frobnicate('x')"), ["Unrecognized function 'frobnicate'"]);
  assert.deepEqual(told("A.Width > 5"), ["Missing units for '5'| (must be mm, in, mil, deg, fs, or ps)"]);
  assert.deepEqual(told("A.memberOfGroup('')"), ["Missing group name argument to memberOfGroup()."], "an empty argument is caught by the preflight");
  assert.equal(compilePcbExpr("A.Nope == 1", env).ok, true);
  assert.equal(compilePcbExpr("A.Width >", env).ok, false);
  assert.deepEqual(told("A.Width >"), ["Syntax error"]);
  assert.equal(compilePcbExpr("A.Width > 0.1mm", env).errors.length, 0);
});

test("an empty expression matches everything, an unknown property matches nothing", () => {
  assert.equal(ids("").length, 23, "every item that is on a layer: 6 tracks, 3 vias, 2 zones, 3 footprints, 6 pads, 2 shapes and a text");
  assert.deepEqual(ids("A.Nope == 1"), []);
  assert.deepEqual(ids("A.Nope != 1"), []);
});

// ---------------------------------------------------------------------------------------------------------------------------------- selecting

test("selecting: the items an expression is true for on any layer they are on", () => {
  assert.deepEqual(ids("A.Type == 'Via'"), ["via_a", "via_b", "via_c"]);
  assert.deepEqual(ids("A.Type == 'Via' && A.Diameter < 0.7mm"), ["via_a"]);
  assert.deepEqual(ids("A.Type == 'Track' && A.NetName == 'GND'"), ["trk_a", "trk_c", "trk_arc"]);
  assert.deepEqual(ids("A.Type == 'Track' && A.Layer == 'B.Cu'"), ["trk_c", "trk_arc"]);
  assert.deepEqual(ids("A.Type == 'Pad' && A.Pad_Type == 'Through-hole'"), ["U1.2"]);
  assert.deepEqual(ids("A.Type == 'Footprint' && A.Layer == 'B.Cu'"), ["R1"]);
  assert.deepEqual(ids("A.Type == 'Track' && A.hasNetclass('Fast')"), ["trk_p", "trk_n"]);
  // || binds tighter than &&: this is  Track && (Via || Pad)  which is nothing
  assert.deepEqual(ids("A.Type == 'Track' && A.Type == 'Via' || A.Type == 'Pad'"), []);
  assert.deepEqual(ids("A.existsOnLayer('B.Cu') && A.Type == 'Track'"), ["trk_c", "trk_arc"]);
});
