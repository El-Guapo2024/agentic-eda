import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { DEFAULT_SELECTION_FILTER } from "../components/canvas/selectionCandidates";
import { CLEARANCE_KINDS, inspectionPick, refineInspectionPick, type InspectPickContext } from "./inspectPick";

const pad = (num: string, x: number, y: number, w = 600, h = 600): Pad => ({ num, net: null, x, y, w, h, round: false, th: false });
const part = (ref: string, pads: Pad[]): Part => ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [4000, 2000], at: [0, 0], rot: 0, side: "top", label: "above", courtyard: [-2000, -1000, 2000, 1000], pads }) as Part;
const board = (parts: Part[], routing: unknown = null): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts, rules: [], routing, drawings: null, checks: [], activity: [], job: "idle" }) as unknown as BoardState;
const ctx: InspectPickContext = { filter: DEFAULT_SELECTION_FILTER, layerVisible: {}, activeLayer: null, highContrast: false, selection: new Set(), onePixelUm: 10, toleranceUm: 150 };
const u1 = part("U1", [pad("1", -1000, 0), pad("2", 1000, 0)]);

test("a click on a pad inspects the pad, a click beside the pads the footprint", () => {
  assert.equal(inspectionPick(board([u1]), -1000, 0, ctx, CLEARANCE_KINDS), "U1.1");
  assert.equal(inspectionPick(board([u1]), 0, -500, ctx, CLEARANCE_KINDS), "U1");
  assert.equal(inspectionPick(board([u1]), 9000, 9000, ctx, CLEARANCE_KINDS), null);
});

test("a footprint the canvas picked gives way to the track or via under the click", () => {
  const routing = { tracks: [{ id: "t1", net: "A", layer: "F.Cu", width: 200, pts: [[-500, 500], [500, 500]] }], vias: [], zones: [], track_width_presets: [], via_presets: [], teardrop_settings: {} };
  const b = board([u1], routing);
  assert.equal(refineInspectionPick(b, "U1", { x: 0, y: 500 }, ctx, CLEARANCE_KINDS), "t1");
  // nothing but the footprint there: it stays
  assert.equal(refineInspectionPick(b, "U1", { x: 0, y: -600 }, ctx, CLEARANCE_KINDS), "U1");
  // what is not a footprint is not looked at again
  assert.equal(refineInspectionPick(b, "t1", { x: 0, y: 500 }, ctx, CLEARANCE_KINDS), "t1");
});
