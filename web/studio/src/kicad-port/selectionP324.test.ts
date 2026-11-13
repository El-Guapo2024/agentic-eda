// UI-LEFT-3 P3-24: `FilterCollectorForFootprints`, a text's area with `ERROR_INSIDE`, a padstack group turned off the quarter turns by its own shape.
// Cases worked from pcbnew/tools/pcb_selection_tool.cpp (`FilterCollectorForFootprints`, `selectPoint`), pcbnew/footprint.cpp (`GetLayerBoundingBox`,
// `HitTestOnLayer`, `GetCoverageArea`), pcbnew/pcb_text.cpp (`TransformTextToPolySet`) and libs/kimath convert_basic_shapes_to_polygon.cpp
// (`TransformOvalToPolygon`) at 8303b2ad.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { filterCollectorForFootprints, viewportBox, type SelectionCandidate } from "../components/canvas/selectionCandidates";
import { padHitDistance } from "./padHit";
import { stadiumRing, ringArea } from "./coverage";

const board = (o: Partial<BoardState>): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...o }) as BoardState;
const padAt = (x: number, y: number): Pad => ({ num: "1", net: null, x, y, w: 1_000_000, h: 1_000_000, round: false, th: false, shape: "rect", size: [1_000_000, 1_000_000], rot: 0 }) as Pad;
// a bottom footprint: two pads 10 mm apart, a silkscreen line along its top on B.SilkS
const part = (ref: string): Part =>
  ({
    ref,
    placed: true,
    side: "bottom",
    at: [0, 0],
    rot: 0,
    courtyard: [-6_000_000, -3_000_000, 6_000_000, 3_000_000],
    pads: [padAt(-5_000_000, 0), padAt(5_000_000, 0)],
    graphics: [{ id: "g", kind: "segment", layer: "B.SilkS", stroke_width: 150_000, start: [-6_000_000, -2_000_000], end: [6_000_000, -2_000_000] }],
  }) as unknown as Part;
const cand = (id: string): SelectionCandidate => ({ kind: "part", id, slopNm: 0, areaNm2: 1, layer: null });
const ids = (c: readonly SelectionCandidate[]) => c.map((x) => x.id);
const allVisible: Record<string, boolean> = {};
const view = (o: Partial<{ viewport: [number, number, number, number] | null; modifying: boolean; selection: Set<string> }> = {}) => ({ viewport: null, modifying: false, selection: new Set<string>(), ...o });

test("FilterCollectorForFootprints: a footprint whose own copper layer is shown always stays", () => {
  const b = board({ parts: [part("U1")] });
  // B.Cu is visible: kept wherever the click is in its courtyard
  assert.deepEqual(ids(filterCollectorForFootprints(b, [cand("U1")], 0, 2_500_000, allVisible, null, false, view())), ["U1"]);
});

test("with its layer not shown (High Contrast on F.Cu): out of the box of its items on the shown layers, or none of them in view, it goes", () => {
  const b = board({ parts: [part("U1")] });
  // High Contrast with F.Cu active: the shown layer is F.Cu alone; the bottom footprint's pads (B.Cu, B.Mask, B.Paste) and silk are on none -> no box
  assert.deepEqual(ids(filterCollectorForFootprints(b, [cand("U1")], 0, 0, allVisible, "F.Cu", true, view())), []);
  // High Contrast on B.SilkS: its silk line is shown, so its box is the line's: a click on the line keeps it, one below goes
  assert.deepEqual(ids(filterCollectorForFootprints(b, [cand("U1")], 0, -2_000_000, allVisible, "b_silks", true, view())), ["U1"]);
  assert.deepEqual(ids(filterCollectorForFootprints(b, [cand("U1")], 0, 1_000_000, allVisible, "b_silks", true, view())), []);
  // the line out of the view: it goes
  assert.deepEqual(ids(filterCollectorForFootprints(b, [cand("U1")], 0, -2_000_000, allVisible, "b_silks", true, view({ viewport: [20_000_000, 20_000_000, 30_000_000, 30_000_000] }))), []);
  // other kinds pass untouched
  const t: SelectionCandidate = { kind: "track", id: "t", slopNm: 0, areaNm2: 1, layer: "F.Cu" };
  assert.deepEqual(ids(filterCollectorForFootprints(b, [t], 0, 0, allVisible, "F.Cu", true, view())), ["t"]);
});

test("with a selection and no modifier a direct hit is needed, unless every selected item is of that footprint", () => {
  // B.Cu hidden: the shown items are the pads on B.Mask / B.Paste and the silk line, so the box spans x -5.5..5.5 mm, y -2.075..0.5 mm
  const hidden: Record<string, boolean> = { "B.Cu": false };
  const b = board({ parts: [part("U1")] });
  const go = (x: number, y: number, o: Parameters<typeof view>[0]) => ids(filterCollectorForFootprints(b, [cand("U1")], x, y, hidden, null, false, view(o)));
  assert.deepEqual(go(0, 0, {}), ["U1"], "nothing selected: inside the box is enough");
  assert.deepEqual(go(0, 0, { selection: new Set(["T1"]) }), [], "a selection made without a modifier: between the pads and off the line is no direct hit");
  assert.deepEqual(go(0, 0, { selection: new Set(["T1"]), modifying: true }), ["U1"], "Shift / Ctrl clicks need none");
  assert.deepEqual(go(5_000_000, 0, { selection: new Set(["T1"]) }), ["U1"], "on a pad: a direct hit");
  assert.deepEqual(go(0, -2_000_000, { selection: new Set(["T1"]) }), ["U1"], "on the silk line: a direct hit");
  assert.deepEqual(go(0, 0, { selection: new Set(["U1.1"]) }), ["U1"], "a pad of its own selected: no direct hit needed");
  assert.deepEqual(go(0, 2_000_000, {}), [], "outside the box of its shown items");
});

test("viewportBox: the board box the canvas shows", () => {
  assert.deepEqual(viewportBox({ x: 100, y: 50, scale: 0.001 }, 800, 600), [-100_000, -50_000, 700_000, 550_000]);
});

test("TransformOvalToPolygon's ERROR_INSIDE: the stadium's chords inside the arcs, so its area is under the exact one; ERROR_OUTSIDE over it", () => {
  const r = 200_000;
  const exact = 1_000_000 * 2 * r + Math.PI * r * r;
  const inside = ringArea(stadiumRing([0, 0], [1_000_000, 0], 2 * r, 5_000, true));
  const outside = ringArea(stadiumRing([0, 0], [1_000_000, 0], 2 * r, 5_000));
  assert.ok(inside < exact && exact < outside, `${inside} < ${exact} < ${outside}`);
});

test("a padstack group turned off the quarter turns is its own shape turned, not its board box", () => {
  // a 2 x 0.4 mm inner-layer group of a pad turned 45 degrees (its board box about 1.7 mm square)
  const s = Math.SQRT1_2;
  const box = Math.round((2_000_000 + 400_000) * s);
  const p = { num: "1", net: null, x: 0, y: 0, w: 100_000, h: 100_000, round: true, th: true, shape: "circle", size: [100_000, 100_000], rot: 45, stack_layers: [{ layers: ["In1.Cu"], role: "inner", x: 0, y: 0, w: box, h: box, round: false, shape: "rect", size: [2_000_000, 400_000] }] } as unknown as Pad;
  assert.ok(padHitDistance(p, 700_000, 700_000) <= 0, "along the turned long axis");
  assert.ok(padHitDistance(p, 700_000, -700_000) > 0, "across it, inside the box: a miss (it was a hit by the box)");
});

test("FilterCollectorForFootprints counts a footprint's zones (box, view and direct hit) and its points (box only)", () => {
  // B.Cu, B.SilkS hidden: the pads (B.Cu/B.Mask/B.Paste: the masks still shown) and the silk; a keepout zone of U1 on F.Cu, a point on F.Fab
  const hidden: Record<string, boolean> = { "B.Cu": false, b_mask: false, b_paste: false, b_silks: false, "B.Mask": false, "B.Paste": false, "B.SilkS": false };
  const zone = { id: "z1", net: "", parent: "U1", layer: "F.Cu", layers: ["F.Cu"], is_rule_area: true, outline: [[-1_000_000, 4_000_000], [1_000_000, 4_000_000], [1_000_000, 6_000_000], [-1_000_000, 6_000_000]] };
  const p = { ...part("U1"), items: { points: [{ id: "U1~p", x: 8_000_000, y: 0, size: 1_000_000, layer: "F.Fab" }] } } as unknown as Part;
  const b = board({ parts: [p], routing: { tracks: [], vias: [], zones: [zone], track_width_presets: [], via_presets: [], teardrop_settings: {} } as never });
  const go = (x: number, y: number, o: Parameters<typeof view>[0] = {}) => ids(filterCollectorForFootprints(b, [cand("U1")], x, y, hidden, null, false, view(o)));
  assert.deepEqual(go(0, 5_000_000), ["U1"], "inside its zone");
  assert.deepEqual(go(7_500_000, 0), ["U1"], "the point stretches the box to x 8.5 mm");
  assert.deepEqual(go(0, 5_000_000, { selection: new Set(["T1"]) }), ["U1"], "a direct hit on its zone");
  assert.deepEqual(go(7_500_000, 0, { selection: new Set(["T1"]) }), [], "the point is no direct hit (`HitTestOnLayer` does not count points)");
  assert.deepEqual(go(0, 5_000_000, { viewport: [20_000_000, 20_000_000, 30_000_000, 30_000_000] }), [], "nothing of it in the view");
});
