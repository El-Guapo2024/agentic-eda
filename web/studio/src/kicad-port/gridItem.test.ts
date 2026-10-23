import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardGridItem } from "../api/pcbItemTypes";
import {
  alignWithGridItems,
  autoSparsePitch,
  findActiveGridAt,
  geometryContains,
  gridBounds,
  gridDialogCorners,
  gridFromCornerSize,
  gridFromCorners,
  gridGeometry,
  gridItemCmd,
  gridLattice,
  gridOutline,
  gridsInPrecedence,
  newGridItem,
  sizeGridToCursor,
  snapToGridItem,
} from "./gridItem";

const grid = (over: Partial<BoardGridItem> = {}): BoardGridItem => ({ ...newGridItem(0, 0), id: "grd_a", ...over });

test("a cartesian grid snaps to its own pitch about its centre, and no further than its extent (GRID_GEOMETRY::Snap)", () => {
  const g = grid({ x: 10_000, y: 0, extent: [3_000, 2_000], spacing: [500, 500] });
  assert.deepEqual(snapToGridItem(g, [10_740, 260]), [10_500, 500]);
  assert.deepEqual(snapToGridItem(g, [20_000, 0]), [13_000, 0], "clamped to the last line");
});

test("a turned grid snaps along its own axes", () => {
  // 90 degrees counter-clockwise on the screen: the grid's x axis points up.
  const g = grid({ extent: [5_000, 5_000], spacing: [1_000, 2_000], angle: 90 });
  // 2 mm up is 2 mm along the grid's x; 1.6 mm right is 1.6 mm along its y, which is nearest the line one pitch (2 mm) out.
  const s = snapToGridItem(g, [1_600, -2_000]);
  assert.deepEqual(s, [2_000, -2_000]);
});

test("a polar grid snaps to its rings and spokes, inside its sweep", () => {
  const g = grid({ kind: "polar", extent: [10_000, 0], spacing: [2_000, 0], phi_extent: 90, phi_spacing: 45 });
  const s = snapToGridItem(g, [2_900, 2_900]);
  // r = 4.1 mm -> 4 mm; phi = 45 degrees -> 45.
  assert.deepEqual(s, [2_828, 2_828]);
  const geom = gridGeometry(g);
  assert.equal(geometryContains(geom, [5_000, 5_000]), true);
  assert.equal(geometryContains(geom, [-5_000, 5_000]), false, "outside the quarter it sweeps");
});

test("the active grid at a point is the one of highest priority, then the smaller (FindActiveGridAt)", () => {
  const big = grid({ id: "big", extent: [10_000, 10_000] });
  const small = grid({ id: "small", extent: [2_000, 2_000] });
  const high = grid({ id: "high", extent: [10_000, 10_000], priority: 3, affects: { cursor: false, routing: true, placement: true } });
  assert.equal(findActiveGridAt([big, small], [0, 0], "cursor")?.id, "small");
  assert.equal(findActiveGridAt([big, small, high], [0, 0], "cursor")?.id, "small", "a grid that does not affect the cursor is not the cursor's");
  assert.equal(findActiveGridAt([big, small, high], [0, 0], "routing")?.id, "high");
  assert.equal(findActiveGridAt([big, small], [0, 0], "cursor", new Set(["small"]))?.id, "big", "a selected grid is being edited");
  assert.deepEqual(gridsInPrecedence([big, small, high]).map((g) => g.id), ["high", "small", "big"]);
});

test("Align: a covering grid owns the snap, a nearby one offers its point within the snap range, else the editor grid stands", () => {
  const g = grid({ x: 0, y: 0, extent: [2_000, 2_000], spacing: [300, 300] });
  assert.deepEqual(alignWithGridItems([g], [610, 0], [1_000, 0], 500), [600, 0]);
  // Just outside, within the snap range of its last line.
  assert.deepEqual(alignWithGridItems([g], [2_150, 0], [2_500, 0], 500), [1_800, 0]);
  assert.deepEqual(alignWithGridItems([g], [5_000, 0], [5_000, 0], 500), [5_000, 0]);
});

test("the outline and box are turned with the grid (buildOutlineWorld)", () => {
  const g = grid({ extent: [2_000, 1_000], angle: 90 });
  assert.deepEqual(gridOutline(g), [[-1_000, 2_000], [-1_000, -2_000], [1_000, -2_000], [1_000, 2_000]]);
  assert.deepEqual(gridBounds(g), [-1_000, -2_000, 1_000, 2_000]);
});

test("Place Sub-grid sizes the grid to the cursor and turns it to face it", () => {
  const g = sizeGridToCursor(newGridItem(0, 0), [0, -3_000]);
  assert.deepEqual([g.extent, g.angle], [[3_000, 3_000], 90], "up on the screen is 90 degrees counter-clockwise");
});

test("the dialog's corners are on the turned outline, and a corner and a size give the centre back", () => {
  const g = grid({ x: 5_000, y: 5_000, extent: [2_000, 1_000], angle: 0 });
  assert.deepEqual(gridDialogCorners(g), { start: [3_000, 4_000], end: [7_000, 6_000], size: [4_000, 2_000] });
  assert.deepEqual(gridFromCorners(g, [3_000, 4_000], [9_000, 6_000]).x, 6_000);
  const turned = grid({ angle: 90 });
  const back = gridFromCornerSize(turned, gridDialogCorners({ ...turned, x: 0, y: 0, extent: [2_000, 1_000] }).start, [4_000, 2_000]);
  assert.deepEqual([back.x, back.y, back.extent], [0, 0, [2_000, 1_000]]);
});

test("the lattice thins a grid too fine for the screen by its tick (gridLattice, AutoSparsePitch)", () => {
  assert.equal(autoSparsePitch(100, 10, 250), 1_000);
  assert.equal(autoSparsePitch(100, 0, 250), 400, "no tick doubles");
  const l = gridLattice(gridGeometry(grid({ spacing: [100, 200] })), 0, 0.05, 10, 10);
  // 10 px at 0.05 px per um is 200 um: the smaller pitch 100 is thinned by 10, the cells keep their shape.
  assert.deepEqual(l.pitch, [1_000, 2_000]);
});

test("the command carries every field and the custom properties", () => {
  const c = gridItemCmd(grid({ custom_properties: [["a", "b"]] }));
  assert.deepEqual(c.at, { x: 0, y: 0 });
  assert.deepEqual(c.custom_properties, [["a", "b"]]);
  assert.equal(gridItemCmd(grid()).custom_properties, undefined);
});
