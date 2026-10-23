import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, FieldInfo, Pad, Part } from "../api/types";
import { footprintBox, MIN_HALF_SIZE_NM } from "./footprintBox";
import { itemBounds } from "./pcbItems";
import { planAlignSelection } from "./alignDistribute";
import { rotationPivot, flipPivot } from "./pcbTransform";

const pad = (num: string, x: number, y: number, w = 600_000, h = 600_000): Pad => ({ num, net: null, x, y, w, h, round: false, th: false });
const field = (name: string, x: number, y: number, extra: Partial<FieldInfo> = {}): FieldInfo =>
  ({ id: `U1:${name}`, name, text: name === "Reference" ? "U1" : "10k", x, y, angle: 0, w: 1_000_000, h: 1_000_000, thickness: 150_000, layer: "F.SilkS", visible: true, halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: true, knockout: false, lx: 0, ly: 0, langle: 0, custom: false, ...extra }) as FieldInfo;
const part = (extra: Partial<Part>): Part => ({ ref: "U1", value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: null, at: [0, 0], rot: 0, side: "top", label: "above", ...extra }) as Part;
const board = (parts: Part[]): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100_000, parts, rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle" }) as unknown as BoardState;

test("an unplaced footprint has no box; a placed one with nothing else is the 0.25 mm square round its position", () => {
  assert.equal(footprintBox(part({ placed: false, at: undefined }), true), null);
  assert.deepEqual(footprintBox(part({ at: [1_000_000, 2_000_000] }), false), [1_000_000 - MIN_HALF_SIZE_NM, 2_000_000 - MIN_HALF_SIZE_NM, 1_000_000 + MIN_HALF_SIZE_NM, 2_000_000 + MIN_HALF_SIZE_NM]);
});

test("the box joins the courtyard, the F.Fab body and every pad: a pad outside the courtyard widens it", () => {
  const p = part({ at: [0, 0], courtyard: [-2_000_000, -1_000_000, 2_000_000, 1_000_000], body: [-1_500_000, -800_000, 1_500_000, 800_000], pads: [pad("1", -1_000_000, 0), pad("2", 3_000_000, 0, 1_000_000, 600_000)] });
  assert.deepEqual(footprintBox(p, false), [-2000, -1000, 3500, 1000], "the courtyard stood in for it before; a connector pin past it counts");
  // without a courtyard the pads are the box (and the position's square)
  const bare = part({ at: [0, 0], pads: [pad("1", -1_000_000, 0), pad("2", 1_000_000, 0)] });
  assert.deepEqual(footprintBox(bare, false), [-1_300_000, -300_000, 1_300_000, 300_000], "pads at +-1000 of 600 wide: out to 1300");
});

test("text is in the box only when asked for, and a hidden field is never in it", () => {
  const p = part({ at: [0, 0], courtyard: [-1_000_000, -500_000, 1_000_000, 500_000], pads: [], fields: [field("Reference", 0, -3_000_000), field("Value", 0, 3_000_000, { visible: false })] });
  const box = (f: FieldInfo): readonly [number, number, number, number] => [f.x - 500_000, f.y - 500_000, f.x + 500_000, f.y + 500_000];
  assert.deepEqual(footprintBox(p, false, box), [-1_000_000, -500_000, 1_000_000, 500_000], "`GetBoundingBox( false )` leaves the text out");
  assert.deepEqual(footprintBox(p, true, box), [-1_000_000, -3_500_000, 1_000_000, 500_000], "`GetBoundingBox( true )` has the visible Reference above it, not the hidden Value");
  assert.deepEqual(footprintBox(p, true), [-1_000_000, -500_000, 1_000_000, 500_000], "a caller that cannot measure text gets the box without it");
});

test("a selection's centre joins the text-included boxes; Align uses the text-excluded one", () => {
  const u1 = part({ ref: "U1", at: [0, 0], courtyard: [-1_000_000, -500_000, 1_000_000, 500_000], pads: [pad("1", 0, 0)], fields: [field("Reference", 0, -4_000_000)] });
  const u2 = part({ ref: "U2", at: [6_000_000, 0], courtyard: [5_000_000, -500_000, 7_000_000, 500_000], pads: [pad("1", 6_000_000, 0)] });
  const b = board([u1, u2]);
  const withText = itemBounds(b, "U1")!;
  assert.ok(withText[1] < -3_000_000, "the Reference above U1 is in its box");
  assert.deepEqual(itemBounds(b, "U1", { includeText: false }), [-1_000_000, -500_000, 1_000_000, 500_000]);
  // Flip's reference point is the centre of the union of the boxes: the text pulls it up
  const [, cy] = flipPivot(b, ["U1", "U2"])!;
  assert.ok(cy < -500_000, `the centre sits above the footprints: ${cy}`);
  assert.equal(rotationPivot(b, ["U1", "U2"])![1], cy, "and Rotate's, which has no snap function here, is the same point");
  // Align Top by the text-excluded boxes: both courtyards start at -500, so nothing needs to move
  assert.deepEqual(planAlignSelection(b, ["U1", "U2"], "top"), []);
});
