import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { modificationAnchor, modificationSnap, type ModificationView } from "./modificationAnchor";
import { DEFAULT_PCB_MAGNETIC } from "./snapAnchors";
import { modificationPoint, planRotate, rotationPivot } from "./pcbTransform";

const pad = (num: string, x: number, y: number, w = 600, h = 600): Pad => ({ num, net: null, x, y, w, h, round: false, th: false });
const part = (ref: string, at: [number, number], pads: Pad[] = []): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1000, at[1] - 500, at[0] + 1000, at[1] + 500], pads }) as Part;
const board = (parts: Part[]): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts, rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle" }) as unknown as BoardState;
// 0.01 px per um: the snap range is 25 px = 2500 um, a 1 mm grid.
const view = (b: BoardState, extra: Partial<ModificationView> = {}): ModificationView => ({ board: b, scale: 0.01, gridUm: 1000, grids: [1000], overrides: null, origin: [0, 0], magnetic: DEFAULT_PCB_MAGNETIC, layerVisible: {}, highContrast: false, activeLayer: null, ...extra });

test("with nothing near, the reference point goes to the nearest grid point (the grid half)", () => {
  const b = board([part("U1", [10_300, 10_300])]);
  assert.deepEqual(modificationAnchor(view(b), [30_240, 30_260]), [30_000, 30_000]);
  assert.deepEqual(modificationAnchor(view(b), [-1_501, 499]), [-2_000, 0], "half a grid away from the other side rounds the way `Align` does");
});

test("a footprint's origin within the snap range wins over the grid point: the anchor half of BestSnapAnchor", () => {
  const b = board([part("U1", [10_300, 10_300])]);
  // the grid point (10 000, 10 000) is 223 um away, the origin 224: the origin is an anchor, and anchors come first
  assert.deepEqual(modificationAnchor(view(b), [10_200, 10_100]), [10_300, 10_300]);
  // a point beyond the range is the grid's
  assert.deepEqual(modificationAnchor(view(b), [14_000, 10_100]), [14_000, 10_000]);
  // the range is 25 screen pixels (less 5 for the hysteresis): zoomed in 10 times (0.1 px/um) an anchor must be within 200 um, the origin 224 away is too far
  assert.deepEqual(modificationAnchor(view(b, { scale: 0.1 }), [10_250, 10_250]), [10_300, 10_300]);
  assert.deepEqual(modificationAnchor(view(b, { scale: 0.1 }), [10_200, 10_100]), [10_000, 10_000], "224 um away is out of the range: the grid's");
  assert.deepEqual(modificationAnchor(view(b, { scale: 0.1 }), [10_600, 10_300]), [11_000, 10_000], "so is 300 um");
});

test("a pad is an anchor only where the magnetic setting says, and only under the point", () => {
  const b = board([part("U1", [10_300, 10_300], [pad("1", 10_500, 10_300)])]);
  // default settings (pads "in the track tool"): the pad is not snapped to by the edit tools, the footprint origin is
  assert.deepEqual(modificationAnchor(view(b), [10_450, 10_350]), [10_300, 10_300]);
  // "always": the pad under the point gives its centre, which is nearer than the origin
  assert.deepEqual(modificationAnchor(view(b, { magnetic: { ...DEFAULT_PCB_MAGNETIC, pads: "always" } }), [10_450, 10_350]), [10_500, 10_300]);
});

test("the snap the planners take: a rotation about several items turns about the anchor their box centre snaps to", () => {
  const b = board([part("U1", [10_300, 10_300]), part("U2", [10_300, 10_300])]);
  const snap = modificationSnap(view(b));
  // two footprints one on the other: the centre of their boxes is (10 300, 10 300), which is an anchor
  assert.deepEqual(modificationPoint(b, ["U1", "U2"], snap), [10_300, 10_300]);
  // U2 sits right of U1 with a courtyard of its own: the union of the boxes is centred at (10 500, 10 150), 269 um from U2's origin and 515 from U1's
  const c = board([part("U1", [10_000, 10_000]), { ...part("U2", [10_400, 10_400]), courtyard: [10_000, 10_000, 12_000, 10_800] }]);
  assert.deepEqual(modificationAnchor(view(c), [10_500, 10_150]), [10_400, 10_400], "the nearest anchor, where the grid point alone would be (11 000, 10 000) or (10 000, 10 000)");
  assert.deepEqual(rotationPivot(c, ["U1", "U2"], modificationSnap(view(c))), [10_400, 10_400]);
  const plan = planRotate(c, ["U1", "U2"], 1, modificationSnap(view(c)), 45);
  assert.deepEqual(plan.cmds[0], { op: "rotate_items", ids: ["U1", "U2"], pivot: { x: 10_400, y: 10_400 }, angle_millideg: -45_000 });
});

test("a single item turns about its own position, snapped to nothing", () => {
  const b = board([part("U1", [10_130, 10_070])]);
  assert.deepEqual(modificationPoint(b, ["U1"], modificationSnap(view(b))), [10_130, 10_070]);
});
