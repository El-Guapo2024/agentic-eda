import { test } from "node:test";
import assert from "node:assert/strict";
import type { Stackup, StackupLayer } from "../api/types";
import { defaultStackup, newStackupSettings, withCopperLayers } from "./boardSetupRules";
import {
  addDielectricChoices,
  addDielectricSublayer,
  adjustDielectricThickness,
  adjustThicknessPrompt,
  canRemoveDielectric,
  itemsOnRemovedLayers,
  removeDielectricChoices,
  removeDielectricSublayer,
  setDefaultLayerWidths,
  stackupEntryErrors,
  stackupItemType,
  stackupRows,
  stackupTotalThicknessNm,
  sublayerOf,
  withCopperLayerCount,
  withSublayer,
  addMaterialsInUse,
  applySubstrate,
  buildStackupReport,
  colorIndex,
  colorToHexString,
  defaultUserColor,
  findSubstrate,
  flipCopperLayer,
  iconColorCss,
  kicadPrms,
  materialDialogResult,
  materialDialogStart,
  newMaterialLists,
  padHasExplicitLayer,
  removedLayerItems,
  withKicadValues,
  type RemovedLayerBoard,
} from "./physicalStackup";

const layer = (s: Stackup, name: string): StackupLayer => s.layers.find((l) => l.name === name)!;
const withLayer = (s: Stackup, name: string, f: (l: StackupLayer) => StackupLayer): Stackup => ({ ...s, layers: s.layers.map((l) => (l.name === name ? f(l) : l)) });
const nm = (mm: number) => Math.round(mm * 1e6);

/** A 4-layer stackup whose first dielectric has three sublayers. */
function threeSublayers(): Stackup {
  return withLayer(defaultStackup(4, 1_600_000), "dielectric 1", (l) => ({ ...l, sublayers: [{ thickness_mm: 0.1, material: "PTFE", epsilon_r: 2.1, loss_tangent: 0.0002 }, { thickness_mm: 0.05 }] }));
}

test("the layer types are the parser's, from the layer's name", () => {
  assert.deepEqual(["F.SilkS", "F.Silkscreen", "F.Paste", "B.Mask", "In7.Cu", "B.Cu", "dielectric 3"].map(stackupItemType), ["silkscreen", "silkscreen", "solderpaste", "soldermask", "copper", "copper", "dielectric"]);
});

test("a row for each sublayer, named as lazyBuildRowUI names it; the Core/PrePreg choice on the first only", () => {
  const rows = stackupRows(threeSublayers(), (f) => (f === "F.SilkS" ? "F.Silkscreen" : f));
  assert.equal(rows.length, 13 + 2, "the default 4-layer stackup's 13 layers, and two more sublayers");
  const d1 = rows.filter((r) => r.dielectricId === 1);
  assert.deepEqual(d1.map((r) => r.label), ["Dielectric 1  (1/3)", "Dielectric 1  (2/3)", "Dielectric 1  (3/3)"]);
  assert.deepEqual(d1.map((r) => r.typeLabel), ["choice", "", ""]);
  assert.deepEqual(rows.filter((r) => r.dielectricId === 2).map((r) => [r.label, r.typeLabel]), [["Dielectric 2", "choice"]], "one sublayer: no (k/n)");
  assert.equal(rows[0]!.label, "F.Silkscreen", "another layer is named as the board names it");
  assert.equal(rows[0]!.typeLabel, "Top Silk Screen");
  assert.equal(rows.find((r) => r.label === "In1.Cu")!.typeLabel, "Copper");
});

test("Add Dielectric Layer lists every dielectric row and inserts DIELECTRIC_PRMS() after the chosen one", () => {
  const plain = defaultStackup(4, 1_600_000);
  assert.deepEqual(addDielectricChoices(plain).map((c) => c.label), ["Dielectric 1", "Dielectric 2", "Dielectric 3"]);
  const s = threeSublayers();
  assert.deepEqual(addDielectricChoices(s).map((c) => c.label), ["Layer 'Dielectric 1' (sublayer 1/3)", "Layer 'Dielectric 1' (sublayer 2/3)", "Layer 'Dielectric 1' (sublayer 3/3)", "Dielectric 2", "Dielectric 3"]);
  // after sublayer 1 of 3: the new one is the second
  const c = addDielectricChoices(s)[0]!;
  const added = addDielectricSublayer(s, c.layer, c.sub);
  const d1 = layer(added, "dielectric 1");
  assert.equal(d1.sublayers!.length, 3);
  assert.deepEqual(d1.sublayers![0], { thickness_mm: 0, epsilon_r: 1, loss_tangent: 0 }, "no material, thickness 0, epsilon R 1, loss tangent 0");
  assert.equal(d1.sublayers![1]!.material, "PTFE", "the old second sublayer is third now");
  assert.equal(layer(s, "dielectric 1").sublayers!.length, 2, "nothing is changed in place");
  // on a dielectric of one sublayer: a second one
  const d2 = addDielectricChoices(plain).find((x) => x.label === "Dielectric 2")!;
  assert.equal(layer(addDielectricSublayer(plain, d2.layer, d2.sub), "dielectric 2").sublayers!.length, 1);
});

test("Remove Dielectric Layer lists the sublayers of the dielectrics that have several, and is off when none has", () => {
  const plain = defaultStackup(4, 1_600_000);
  assert.equal(canRemoveDielectric(plain), false, "onRemoveDielUI: nothing to remove");
  assert.deepEqual(removeDielectricChoices(plain), []);
  const s = threeSublayers();
  assert.equal(canRemoveDielectric(s), true);
  assert.deepEqual(removeDielectricChoices(s).map((c) => c.label), ["Layer 'Dielectric 1' sublayer 1/3", "Layer 'Dielectric 1' sublayer 2/3", "Layer 'Dielectric 1' sublayer 3/3"]);
  // the second goes
  const second = removeDielectricChoices(s)[1]!;
  assert.deepEqual(layer(removeDielectricSublayer(s, second.layer, second.sub), "dielectric 1").sublayers, [{ thickness_mm: 0.05 }]);
  // the first goes: the next is the layer's own now, the type stays the layer's
  const d1 = layer(removeDielectricSublayer(s, removeDielectricChoices(s)[0]!.layer, 0), "dielectric 1");
  assert.deepEqual([d1.material, d1.thickness_mm, d1.epsilon_r, d1.loss_tangent, d1.kind], ["PTFE", 0.1, 2.1, 0.0002, "core"]);
  assert.deepEqual(d1.sublayers, [{ thickness_mm: 0.05 }]);
  // the last of a layer cannot go (`GetSublayersCount() < 2`)
  const one = removeDielectricSublayer(plain, plain.layers.findIndex((l) => l.name === "dielectric 1"), 0);
  assert.deepEqual(one, plain);
});

test("a sublayer's fields are edited one at a time; the layer's own are sublayer 0", () => {
  const s = threeSublayers();
  const d1 = layer(s, "dielectric 1");
  assert.equal(sublayerOf(d1, 0).thickness_mm, d1.thickness_mm);
  const locked = withSublayer(withSublayer(d1, 2, { thickness_locked: true, color: "#11223344" }), 0, { thickness_locked: true });
  assert.equal(locked.thickness_locked, true);
  assert.deepEqual(locked.sublayers![1], { thickness_mm: 0.05, thickness_locked: true, color: "#11223344" });
  assert.equal(withSublayer(locked, 0, { thickness_locked: false }).thickness_locked, undefined, "unlocked is not written");
  assert.equal(stackupTotalThicknessNm(s), 1_600_000 + nm(0.15), "BuildBoardThicknessFromStackup counts every sublayer");
});

test("transferDataFromUIToStackup's checks, in its words", () => {
  const ok = threeSublayers();
  assert.equal(stackupEntryErrors(ok), null);
  assert.equal(stackupEntryErrors(withLayer(ok, "dielectric 1", (l) => ({ ...l, epsilon_r: undefined }))), null, "a blank value is not written, not wrong");
  const eps = withLayer(ok, "dielectric 2", (l) => ({ ...l, epsilon_r: -1 }));
  assert.equal(stackupEntryErrors(eps), "Incorrect value for Epsilon R (Epsilon R must be positive or null if not used)");
  const loss = withLayer(ok, "dielectric 1", (l) => withSublayer(l, 1, { loss_tangent: Number.NaN }));
  assert.equal(stackupEntryErrors(loss), "Incorrect value for Loss tg (Loss tg must be positive or null if not used)");
  const thick = withLayer(ok, "In1.Cu", (l) => ({ ...l, thickness_mm: -0.01 }));
  assert.equal(stackupEntryErrors(thick), "A layer thickness is < 0. Fix it");
  // every row adds its own; an epsilon R after another message is appended with no new line, as `error_msg << _( "Incorrect value for Epsilon R .." )` is
  const both = withLayer(withLayer(thick, "dielectric 2", (l) => ({ ...l, epsilon_r: -1 })), "F.Mask", (l) => ({ ...l, loss_tangent: -1 }));
  assert.equal(
    stackupEntryErrors(both),
    "Incorrect value for Loss tg (Loss tg must be positive or null if not used)\nA layer thickness is < 0. Fix it" + "Incorrect value for Epsilon R (Epsilon R must be positive or null if not used)",
  );
});

test("setDefaultLayerWidths: prepreg of 0.1 mm outside, the cores take the rest; a locked dielectric keeps its own", () => {
  const four = defaultStackup(4, 1_600_000);
  const s = setDefaultLayerWidths(four, 4, 1_600_000);
  // fixed: 4 x 0.035 copper + 2 x 0.01 mask = 0.16 mm; 2 prepregs of 0.1; one core of 1.6 - 0.16 - 0.2 = 1.24
  assert.deepEqual(["dielectric 1", "dielectric 2", "dielectric 3"].map((n) => [layer(s, n).kind, nm(layer(s, n).thickness_mm!)]), [["prepreg", 100_000], ["core", 1_240_000], ["prepreg", 100_000]]);
  assert.equal(stackupTotalThicknessNm(s), 1_600_000);
  // a 2-layer board: one core
  const two = setDefaultLayerWidths(defaultStackup(2, 1_600_000), 2, 1_000_000);
  assert.deepEqual([layer(two, "dielectric 1").kind, nm(layer(two, "dielectric 1").thickness_mm!)], ["core", 1_000_000 - 90_000]);
  // too thin for 0.1 mm prepreg and a core: every dielectric gets the same
  const thin = setDefaultLayerWidths(four, 4, 400_000);
  assert.deepEqual(["dielectric 1", "dielectric 2", "dielectric 3"].map((n) => nm(layer(thin, n).thickness_mm!)), [80_000, 80_000, 80_000]);
  // a locked dielectric and a second sublayer count as fixed; the alternation steps over the locked one
  const locked = withLayer(withLayer(four, "dielectric 2", (l) => ({ ...l, thickness_mm: 0.5, thickness_locked: true, kind: "prepreg" })), "dielectric 3", (l) => ({ ...l, sublayers: [{ thickness_mm: 0.04 }] }));
  const after = setDefaultLayerWidths(locked, 4, 1_600_000);
  assert.deepEqual(["dielectric 1", "dielectric 2", "dielectric 3"].map((n) => [layer(after, n).kind, nm(layer(after, n).thickness_mm!)]), [["prepreg", 100_000], ["prepreg", 500_000], ["prepreg", 100_000]]);
  assert.equal(layer(after, "dielectric 3").sublayers![0]!.thickness_mm, 0.04, "a sublayer after the first is not adjusted");
});

test("Adjust Dielectric Thickness: the prompt, the least value and its messages", () => {
  const four = defaultStackup(4, 1_600_000);
  const p = adjustThicknessPrompt(four, "mm");
  assert.equal(p.title, "Adjust Unlocked Dielectric Layers");
  assert.equal(p.minNm, 160_000);
  assert.equal(p.prompt, "Enter expected board thickness (min value 0.16 mm):");
  assert.equal(adjustThicknessPrompt({ layers: four.layers.filter((l) => stackupItemType(l.name) === "dielectric") }, "mm").prompt, "Enter board thickness in mm:");
  assert.deepEqual(adjustDielectricThickness(four, 4, 100_000, "mm"), { stackup: null, message: "Value too small (min value 0.16 mm)." });
  const allLocked: Stackup = { layers: four.layers.map((l) => (stackupItemType(l.name) === "dielectric" ? { ...l, thickness_locked: true } : l)) };
  assert.deepEqual(adjustDielectricThickness(allLocked, 4, 5_000_000, "mm"), { stackup: null, message: "All dielectric  thickness layers are locked" });
  const r = adjustDielectricThickness(four, 4, 2_000_000, "mm");
  assert.equal(r.message, null);
  assert.equal(stackupTotalThicknessNm(r.stackup!), 2_000_000);
});

test("fewer copper layers list what is on the layers that go, as GetItemDescription says it", () => {
  const board: RemovedLayerBoard = {
    routing: {
      tracks: [
        { id: "t1", net: "GND", layer: "In1.Cu", width: 200_000, pts: [[0, 0], [1_000_000, 0], [1_000_000, 2_000_000]] },
        { id: "t2", net: "VCC", layer: "F.Cu", width: 200_000, pts: [[0, 0], [1_000_000, 0]] },
        { id: "a1", net: "", layer: "In2.Cu", width: 200_000, pts: [[0, 0], [1_000_000, 1_000_000], [2_000_000, 0]], arc_mid: [1_000_000, 1_000_000] },
      ],
      vias: [
        { id: "v1", net: "GND", x: 0, y: 0, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" },
        { id: "v2", net: "GND", x: 0, y: 0, d: 600_000, drill: 300_000, from: "F.Cu", to: "In1.Cu", type: "blind" },
        { id: "v3", net: "GND", x: 0, y: 0, d: 600_000, drill: 300_000, from: "In2.Cu", to: "In1.Cu", type: "buried" },
      ],
      zones: [
        { id: "z1", net: "GND", layer: "F.Cu", layers: ["F.Cu", "In1.Cu"], priority: 2, is_rule_area: false, teardrop: false },
        { id: "z2", net: "", layer: "In2.Cu", priority: 0, is_rule_area: true, teardrop: false, name: "keep" },
      ],
    },
    drawings: {
      shapes: [{ id: "s1", kind: "segment", layer: "In2.Cu", stroke_width: 100_000, filled: false, start: [0, 0], end: [1, 1] }],
      texts: [{ id: "x1", content: "${REV}", shown: "B", x: 0, y: 0, angle: 0, layer: "In1.Cu", size: 1_000_000, stroke_width: 150_000, justify: "left", mirror: false }],
      groups: [],
      dimensions: [],
    },
  } as unknown as RemovedLayerBoard;
  assert.deepEqual(itemsOnRemovedLayers(board, 4, 4), [], "no layer goes");
  // a via is listed on every removed layer its drill spans, a through via too (`IsOnLayer`), though it is not deleted
  assert.deepEqual(itemsOnRemovedLayers(board, 4, 2), [
    "PCB text 'B' on In1.Cu",
    "Track [GND] on In1.Cu, length 1.0000 mm",
    "Track [GND] on In1.Cu, length 2.0000 mm",
    "Via [GND] on F.Cu - B.Cu",
    "Blind via [GND] on F.Cu - In1.Cu",
    "Buried via [GND] on In1.Cu - In2.Cu",
    "Zone [GND] on F.Cu and In1.Cu, priority 2",
    "Segment [<no net>] on In2.Cu",
    `Track (arc) [<no net>] on In2.Cu, length ${Math.PI.toFixed(4)} mm`,
    "Via [GND] on F.Cu - B.Cu",
    "Buried via [GND] on In1.Cu - In2.Cu",
    "Rule area 'keep' on In2.Cu",
  ]);
  // a layer with nothing but vias passing through is not a removed layer with items (`HasItemsOnLayer`)
  const vias = { routing: { tracks: [], zones: [], vias: board.routing!.vias.slice(0, 2) }, drawings: null } as unknown as RemovedLayerBoard;
  assert.deepEqual(itemsOnRemovedLayers(vias, 6, 4), [], "In3.Cu and In4.Cu: nothing on them");
  assert.deepEqual(itemsOnRemovedLayers(vias, 4, 2), ["Via [GND] on F.Cu - B.Cu", "Blind via [GND] on F.Cu - In1.Cu"], "In1.Cu has the blind via's end: listed with the through via");
  assert.deepEqual(itemsOnRemovedLayers({ routing: { tracks: [], zones: [], vias: board.routing!.vias.slice(0, 1) }, drawings: null } as unknown as RemovedLayerBoard, 4, 2), [], "a through via alone");
});

test("a new copper layer count shares the board's old thickness among the unlocked dielectrics (onCopperLayersSelCount)", () => {
  const two = newStackupSettings(2, 1_600_000);
  const four = withCopperLayerCount(two, 4, withCopperLayers);
  assert.equal(four.copper_layers, 4);
  assert.equal(stackupTotalThicknessNm(four.stackup), 1_600_000, "the board keeps its 1.6 mm");
  assert.deepEqual(["dielectric 1", "dielectric 2", "dielectric 3"].map((n) => [layer(four.stackup, n).kind, nm(layer(four.stackup, n).thickness_mm!)]), [["prepreg", 100_000], ["core", 1_240_000], ["prepreg", 100_000]]);
  assert.equal(stackupTotalThicknessNm(withCopperLayerCount(four, 2, withCopperLayers).stackup), 1_600_000, "and back");
});

test("a value the board file leaves out is the type's default, as KiCad shows it (BOARD_STACKUP_ITEM( type ), DIELECTRIC_PRMS())", () => {
  const s = threeSublayers();
  const bare = (name: string): StackupLayer => ({ name, material: null, thickness_mm: null });
  const p = (l: StackupLayer, sub = 0) => [kicadPrms(l, sub).material, kicadPrms(l, sub).epsilonR, kicadPrms(l, sub).lossTangent, kicadPrms(l, sub).color, kicadPrms(l, sub).thicknessNm];
  assert.deepEqual(p(bare("dielectric 1")), ["FR4", 4.5, 0.02, "Not specified", 0]);
  assert.deepEqual(p(bare("F.Mask")), ["Not specified", 3.3, 0, "Not specified", 10_000]);
  assert.deepEqual(p(bare("F.SilkS")), ["Not specified", 1, 0, "Not specified", 0]);
  assert.deepEqual(p(bare("In1.Cu")), ["", 1, 0, "", 35_000]);
  assert.deepEqual(p(layer(s, "dielectric 1"), 2), ["", 1, 0, "", 50_000], "a later sublayer: DIELECTRIC_PRMS()");
  assert.deepEqual(p(layer(s, "dielectric 1"), 1), ["PTFE", 2.1, 0.0002, "", 100_000], "what the file says");
});

test("Apply writes every value as transferDataFromUIToStackup does", () => {
  const s: Stackup = {
    layers: [
      { name: "F.Mask", material: null, thickness_mm: 0.01, kind: "Top Solder Mask", color: "Light Green" },
      { name: "dielectric 1", material: null, thickness_mm: 1.5, kind: "core", color: "#11223344", sublayers: [{ thickness_mm: 0.1, color: "Polyimide" }] },
      { name: "B.Mask", material: "Epoxy", thickness_mm: 0.01, kind: "Bottom Solder Mask", epsilon_r: 3.9, color: "Green" },
      { name: "F.Cu", material: null, thickness_mm: 0.035, kind: "copper" },
    ],
  };
  const w = withKicadValues(s);
  assert.deepEqual(w.layers[0], { name: "F.Mask", material: null, thickness_mm: 0.01, kind: "Top Solder Mask", epsilon_r: 3.3, loss_tangent: 0 }, "a name the box has not got is Not specified");
  assert.deepEqual(w.layers[1], { name: "dielectric 1", material: "FR4", thickness_mm: 1.5, kind: "core", color: "#11223344", epsilon_r: 4.5, loss_tangent: 0.02, sublayers: [{ thickness_mm: 0.1, color: "Polyimide", epsilon_r: 1, loss_tangent: 0 }] });
  assert.deepEqual(w.layers[2], { ...s.layers[2], loss_tangent: 0 }, "what is there stays");
  assert.deepEqual(w.layers[3], s.layers[3], "copper has none of them");
});

test("the material list: KiCad's tables, the materials in use added, the dialog's checks, the material's colour", () => {
  const lists = newMaterialLists();
  assert.deepEqual(lists.dielectric.map((m) => m.name), ["Not specified", "FR4", "FR408-HR", "Polyimide", "Kapton", "Polyolefin", "Al", "PTFE", "Teflon", "Ceramic"]);
  assert.deepEqual(lists.soldermask.map((m) => [m.name, m.epsilonR]), [["Not specified", 3.3], ["Epoxy", 3.3], ["Liquid Ink", 3.3], ["Dry Film", 3.3]]);
  assert.deepEqual(lists.silkscreen.map((m) => m.name), ["Not specified", "Liquid Photo", "Direct Printing"]);
  assert.equal(findSubstrate(lists.dielectric, "fr4", 4.5, 0.02), 1, "the name in any case");
  assert.equal(findSubstrate(lists.dielectric, "FR4", 4.4, 0.02), -1, "and the values exactly");

  const s = threeSublayers();
  addMaterialsInUse(lists, s);
  // dielectric 1's PTFE at 2.1 / 0.0002 is in the list; its third sublayer (no material: "Not specified", 1, 0) is not
  assert.deepEqual(lists.dielectric.slice(10), [{ name: "Not specified", epsilonR: 1, lossTangent: 0 }]);
  addMaterialsInUse(lists, s);
  assert.equal(lists.dielectric.length, 11, "added once");
  assert.equal(lists.soldermask.length, 4, "the masks' Not specified 3.3 / 0 is the list's first");
  assert.equal(new Set([newMaterialLists().dielectric, newMaterialLists().dielectric]).size, 2, "each panel its own copy");

  assert.deepEqual(materialDialogStart(), { name: "", epsilonR: "1", lossTangent: "0" });
  assert.deepEqual(materialDialogResult({ name: "X", epsilonR: "abc", lossTangent: "0" }), { error: "Incorrect value for Epsilon R" });
  assert.deepEqual(materialDialogResult({ name: "X", epsilonR: "3", lossTangent: "-1" }), { error: "Incorrect value for Loss Tangent" });
  assert.deepEqual(materialDialogResult({ name: "X", epsilonR: "3.5", lossTangent: "0.01" }), { substrate: { name: "X", epsilonR: 3.5, lossTangent: 0.01 } });

  const d1 = s.layers.findIndex((l) => l.name === "dielectric 1");
  const picked = applySubstrate(s, d1, 2, { name: "Kapton", epsilonR: 3.2, lossTangent: 0.004 });
  assert.deepEqual(sublayerOf(picked.layers[d1]!, 2), { thickness_mm: 0.05, material: "Kapton", epsilon_r: 3.2, loss_tangent: 0.004, color: "Polyimide" });
  const colour = (name: string) => sublayerOf(applySubstrate(s, d1, 0, { name, epsilonR: 1, lossTangent: 0 }).layers[d1]!, 0).color;
  assert.deepEqual(["PTFE", "Teflon", "Al", "FR408-HR", "ptfe"].map(colour), ["PTFE natural", "PTFE natural", "Aluminum", "FR4 natural", "FR4 natural"]);
  const custom = withLayer(s, "dielectric 1", (l) => ({ ...l, color: "#11223344" }));
  assert.equal(sublayerOf(applySubstrate(custom, d1, 0, { name: "PTFE", epsilonR: 2.1, lossTangent: 0.0002 }).layers[d1]!, 0).color, "#11223344", "a user defined colour stays");
  const mask = s.layers.findIndex((l) => l.name === "F.Mask");
  assert.equal(sublayerOf(applySubstrate(s, mask, 0, { name: "Epoxy", epsilonR: 3.3, lossTangent: 0 }).layers[mask]!, 0).color, undefined, "only a dielectric takes its material's colour");
  assert.equal(applySubstrate(s, d1, 0, { name: "", epsilonR: 3, lossTangent: 0 }), s, "no substrate specified");
});

test("the colour box and the row's icon (createColorBox, getColorIconItem)", () => {
  assert.deepEqual([colorIndex("soldermask", "Green"), colorIndex("soldermask", "#00FF00FF"), colorIndex("soldermask", "Light Green"), colorIndex("dielectric", "Aluminum")], [1, 8, 0, 5]);
  assert.deepEqual([defaultUserColor("soldermask"), defaultUserColor("silkscreen"), defaultUserColor("dielectric")], ["#808080D4", "#808080FF", "#808080FF"]);
  assert.deepEqual([colorToHexString("rgb(1, 2, 255)"), colorToHexString("rgba(16, 32, 48, 0.5)"), colorToHexString("#a0b0c0")], ["#0102FFFF", "#10203080", "#A0B0C0FF"]);
  const l = (name: string, color?: string): StackupLayer => ({ name, material: null, thickness_mm: null, ...(color ? { color } : {}) });
  assert.deepEqual(
    [iconColorCss(l("F.Cu"), 0), iconColorCss(l("dielectric 1", "Polyimide"), 0), iconColorCss(l("F.Paste"), 0), iconColorCss(l("F.Mask", "Red"), 0), iconColorCss(l("F.Mask"), 0), iconColorCss(l("B.SilkS", "#10203080"), 0)],
    ["rgb(220, 180, 30)", "rgb(75, 120, 75)", "rgb(200, 200, 200)", "rgb(128, 0, 0)", "rgb(80, 80, 80)", "rgb(16, 32, 48)"]
  );
});

test("Export to Clipboard is BuildStackupReport's text", () => {
  const two = defaultStackup(2, 1_600_000);
  const named = (f: string) => ({ "F.SilkS": "F.Silkscreen", "B.SilkS": "B.Silkscreen" })[f] ?? f;
  assert.equal(
    buildStackupReport({ stackup: two }, "mm", named),
    [
      'layer "F.Silkscreen" type "Top Silk Screen" Color "Not specified" Material "Not specified"',
      'layer "F.Paste" type "Top Solder Paste"',
      'layer "F.Mask" type "Top Solder Mask" Color "Not specified" Thickness 0.01 mm Material "Not specified" EpsilonR 3.3 LossTg 0',
      'layer "F.Cu" type "copper" Thickness 0.035 mm',
      'layer "Dielectric 1" type "core"',
      '  sublayer "1/1" Color "Not specified" Thickness 1.51 mm Material "FR4" EpsilonR 4.5 LossTg 0.02',
      'layer "B.Cu" type "copper" Thickness 0.035 mm',
      'layer "B.Mask" type "Bottom Solder Mask" Color "Not specified" Thickness 0.01 mm Material "Not specified" EpsilonR 3.3 LossTg 0',
      'layer "B.Paste" type "Bottom Solder Paste"',
      'layer "B.Silkscreen" type "Bottom Silk Screen" Color "Not specified" Material "Not specified"',
      'Finish "Not specified"',
      "",
    ].join("\n")
  );
  const s = { ...withLayer(threeSublayers(), "dielectric 1", (l) => ({ ...l, thickness_locked: true, color: "FR4 natural" })), copper_finish: "ENIG", dielectric_constraints: true, edge_plating: true, edge_connector: 2 };
  const lines = buildStackupReport({ stackup: s }, "mil").split("\n");
  const at = lines.indexOf('layer "Dielectric 1" type "core"');
  assert.deepEqual(lines.slice(at, at + 4), [
    'layer "Dielectric 1" type "core"',
    `  sublayer "1/3" Color "FR4 natural" Thickness ${String(Math.round((nm(layer(s, "dielectric 1").thickness_mm!) / 25_400) * 1e5) / 1e5)} mils Locked Material "FR4" EpsilonR 4.5 LossTg 0.02`,
    '  sublayer "2/3" Thickness 3.93701 mils Material "PTFE" EpsilonR 2.1 LossTg 0.0002',
    '  sublayer "3/3" Thickness 1.9685 mils Material "Not specified" EpsilonR 1 LossTg 0',
  ]);
  assert.equal(lines.at(-2), 'Finish "ENIG" Option "Impedance Controlled" Option "Plated edges" EdgeConnector "yes,bevelled"');
});

test("Show Details: each entry the item it selects, and the pads with a shape of their own on a removed layer", () => {
  assert.deepEqual(["F.Cu", "B.Cu", "In1.Cu", "In2.Cu", "In1.Cu"].map((l, i) => flipCopperLayer(l, i < 4 ? 4 : 2)), ["B.Cu", "F.Cu", "In2.Cu", "In1.Cu", "In1.Cu"]);
  const custom = { stack: { mode: "custom" as const, layers: { "In1.Cu": { shape: "circle" as const, size: [1, 1] as [number, number] } } } };
  const fib = { stack: { mode: "front_inner_back" as const, inner: { shape: "circle" as const, size: [1, 1] as [number, number] } } };
  assert.deepEqual(
    [padHasExplicitLayer(custom, "top", "In1.Cu", 4), padHasExplicitLayer(custom, "bottom", "In1.Cu", 4), padHasExplicitLayer(custom, "bottom", "In2.Cu", 4), padHasExplicitLayer(fib, "bottom", "In1.Cu", 4), padHasExplicitLayer(fib, "top", "In2.Cu", 4), padHasExplicitLayer({}, "top", "In1.Cu", 4)],
    [true, false, true, true, false, false],
    "a custom stack's layers flip with a bottom footprint; a front/inner/back stack's inner shape is INNER_LAYERS, In1.Cu"
  );
  const board = {
    routing: { tracks: [{ id: "t1", net: "GND", layer: "In1.Cu", width: 200_000, pts: [[0, 0], [1_000_000, 0], [1_000_000, 2_000_000]] }], vias: [], zones: [] },
    drawings: null,
    parts: [
      { ref: "U1", placed: true, side: "top", pads: [{ num: "1", ...custom }, { num: "1", ...fib }, { num: "2" }] },
      { ref: "U2", placed: false, pads: [{ num: "1", ...custom }] },
    ],
  } as unknown as RemovedLayerBoard;
  assert.deepEqual(removedLayerItems(board, 4, 2), [
    { text: "Track [GND] on In1.Cu, length 1.0000 mm", id: "t1" },
    { text: "Track [GND] on In1.Cu, length 2.0000 mm", id: "t1" },
    { text: "Pad 1 of U1 on In1.Cu", id: "U1.1" },
    { text: "Pad 1 of U1 on In1.Cu", id: "U1.1#2" },
  ]);
  assert.deepEqual(removedLayerItems({ ...board, routing: { tracks: [], vias: [], zones: [] } } as unknown as RemovedLayerBoard, 4, 2), [], "pads alone do not make a removed layer with items (HasItemsOnLayer)");
});
