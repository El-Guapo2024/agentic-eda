import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardGridItem } from "../api/pcbItemTypes";
import type { BoardState } from "../api/types";
import { gridCarryAnchor, gridCarryDrop } from "./placeCarry";
import { newGridItem } from "./gridItem";

const MM = 1_000_000;
const part = (ref: string, x: number) => ({ ref, value: null, placed: true, at: [x, 0], rot: 0, side: "top", courtyard: [x - MM, -MM, x + MM, MM], pads: [{ num: "1", x: x + 500_000, y: 0, w: 400_000, h: 400_000, round: false, th: false, net: null }] });
const board = (grids: BoardGridItem[] = []) => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [part("TP1", 0), part("U2", 20 * MM)], rules: [], routing: null, drawings: { shapes: [], texts: [], dimensions: [], dimension_settings: {}, groups: [], grid_items: grids }, checks: [], activity: [], job: "idle" }) as unknown as BoardState;

test("PlaceFootprint's carry moves the footprint's anchor to the grid point, a lone footprint only", () => {
  assert.deepEqual(gridCarryAnchor(board(), ["TP1"]), [0, 0]);
  assert.equal(gridCarryAnchor(board(), ["TP1", "U2"]), null);
  // a click with no motion: the anchor on the click's grid point (`fp->SetPosition( cursorPos )`), however far a pad of it is from it
  const d = gridCarryDrop(board(), ["TP1"], [3 * MM, 2 * MM], 0, 90)!;
  assert.equal(d.dxNm, 3 * MM);
  assert.equal(d.dyNm, 2 * MM);
  assert.deepEqual(d.grabNm, [0, 0], "R and F turn it about its anchor (`selection().SetReferencePoint( cursorPos )`)");
  assert.equal(d.ops, undefined, "no placement grid: no turn");
});

test("a click with no motion inside a placement grid puts the footprint down turned to the grid's frame (`applyPlacementFrameOrientation` after the pick)", () => {
  // a 30 degree grid round (50, 0) mm that affects placement
  const g: BoardGridItem = { ...newGridItem(50 * MM, 0), id: "grd_a", extent: [10 * MM, 10 * MM], angle: 30 };
  const d = gridCarryDrop(board([g]), ["TP1"], [50 * MM, 0], 0, 90)!;
  assert.equal(d.frameDeg, 30);
  assert.equal(d.ops?.length, 1);
  assert.deepEqual(d.ops?.[0], { kind: "turn", degCcw: 30, pivot: [0, 0] });
  // outside it: unturned
  const out = gridCarryDrop(board([g]), ["TP1"], [5 * MM, 0], 0, 90)!;
  assert.equal(out.ops?.length ?? 0, 0);
  // a footprint that already carries the frame (handed over from another command) is not turned again
  assert.equal(gridCarryDrop(board([g]), ["TP1"], [50 * MM, 0], 30, 90)!.ops?.length ?? 0, 0);
});
