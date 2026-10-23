import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { modificationAnchor, modificationSnap, type ModificationView } from "./modificationAnchor";
import { DEFAULT_PCB_MAGNETIC } from "./snapAnchors";
import { modificationPoint, planRotate, rotationPivot } from "./pcbTransform";

const pad = (num: string, x: number, y: number, w = 600_000, h = 600_000): Pad => ({ num, net: null, x, y, w, h, round: false, th: false });
const part = (ref: string, at: [number, number], pads: Pad[] = []): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2_000_000, 1_000_000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1_000_000, at[1] - 500_000, at[0] + 1_000_000, at[1] + 500_000], pads }) as Part;
const board = (parts: Part[]): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100_000, parts, rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle" }) as unknown as BoardState;
// 0.01 px per um: the snap range is 25 px = 2500 um, a 1 mm grid.
const view = (b: BoardState, extra: Partial<ModificationView> = {}): ModificationView => ({ board: b, scale: 0.00001, gridNm: 1_000_000, grids: [1_000_000], overrides: null, origin: [0, 0], magnetic: DEFAULT_PCB_MAGNETIC, layerVisible: {}, highContrast: false, activeLayer: null, ...extra });

test("with nothing near, the reference point goes to the nearest grid point (the grid half)", () => {
  const b = board([part("U1", [10_300_000, 10_300_000])]);
  assert.deepEqual(modificationAnchor(view(b), [30_240_000, 30_260_000]), [30_000_000, 30_000_000]);
  assert.deepEqual(modificationAnchor(view(b), [-1_501_000, 499_000]), [-2_000_000, 0], "half a grid away from the other side rounds the way `Align` does");
});

test("a footprint's origin within the snap range wins over the grid point: the anchor half of BestSnapAnchor", () => {
  const b = board([part("U1", [10_300_000, 10_300_000])]);
  // the grid point (10 000, 10 000) is 223 um away, the origin 224: the origin is an anchor, and anchors come first
  assert.deepEqual(modificationAnchor(view(b), [10_200_000, 10_100_000]), [10_300_000, 10_300_000]);
  // a point beyond the range is the grid's
  assert.deepEqual(modificationAnchor(view(b), [14_000_000, 10_100_000]), [14_000_000, 10_000_000]);
  // the range is 25 screen pixels (less 5 for the hysteresis): zoomed in 10 times (0.1 px/um) an anchor must be within 200 um, the origin 224 away is too far
  assert.deepEqual(modificationAnchor(view(b, { scale: 0.0001 }), [10_250_000, 10_250_000]), [10_300_000, 10_300_000]);
  assert.deepEqual(modificationAnchor(view(b, { scale: 0.0001 }), [10_200_000, 10_100_000]), [10_000_000, 10_000_000], "224 um away is out of the range: the grid's");
  assert.deepEqual(modificationAnchor(view(b, { scale: 0.0001 }), [10_600_000, 10_300_000]), [11_000_000, 10_000_000], "so is 300 um");
});

test("a pad is an anchor only where the magnetic setting says, and only under the point", () => {
  const b = board([part("U1", [10_300_000, 10_300_000], [pad("1", 10_500_000, 10_300_000)])]);
  // default settings (pads "in the track tool"): the pad is not snapped to by the edit tools, the footprint origin is
  assert.deepEqual(modificationAnchor(view(b), [10_450_000, 10_350_000]), [10_300_000, 10_300_000]);
  // "always": the pad under the point gives its centre, which is nearer than the origin
  assert.deepEqual(modificationAnchor(view(b, { magnetic: { ...DEFAULT_PCB_MAGNETIC, pads: "always" } }), [10_450_000, 10_350_000]), [10_500_000, 10_300_000]);
});

test("the snap the planners take: a rotation about several items turns about the anchor their box centre snaps to", () => {
  const b = board([part("U1", [10_300_000, 10_300_000]), part("U2", [10_300_000, 10_300_000])]);
  const snap = modificationSnap(view(b));
  // two footprints one on the other: the centre of their boxes is (10 300, 10 300), which is an anchor
  assert.deepEqual(modificationPoint(b, ["U1", "U2"], snap), [10_300_000, 10_300_000]);
  // U2 sits right of U1 with a courtyard of its own: the union of the boxes is centred at (10 500, 10 150), 269 um from U2's origin and 515 from U1's
  const c = board([part("U1", [10_000_000, 10_000_000]), { ...part("U2", [10_400_000, 10_400_000]), courtyard: [10_000_000, 10_000_000, 12_000_000, 10_800_000] }]);
  assert.deepEqual(modificationAnchor(view(c), [10_500_000, 10_150_000]), [10_400_000, 10_400_000], "the nearest anchor, where the grid point alone would be (11 000, 10 000) or (10 000, 10 000)");
  assert.deepEqual(rotationPivot(c, ["U1", "U2"], modificationSnap(view(c))), [10_400_000, 10_400_000]);
  const plan = planRotate(c, ["U1", "U2"], 1, modificationSnap(view(c)), 45);
  assert.deepEqual(plan.cmds[0], { op: "rotate_items", ids: ["U1", "U2"], pivot: { x: 10_400_000, y: 10_400_000 }, angle_millideg: -45_000 });
});

test("a single item turns about its own position, snapped to nothing", () => {
  const b = board([part("U1", [10_130_000, 10_070_000])]);
  assert.deepEqual(modificationPoint(b, ["U1"], modificationSnap(view(b))), [10_130_000, 10_070_000]);
});
