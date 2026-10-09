import { test } from "node:test";
import assert from "node:assert/strict";
import { boxesOverlap, courtyardConflicts, movedBox, polygonHitsBox, type MovePreview } from "./courtyardConflicts";
import type { BoardState } from "../api/types";

const part = (ref: string, courtyard: [number, number, number, number], side: "top" | "bottom" = "top") => ({ ref, placed: true, courtyard, at: [(courtyard[0] + courtyard[2]) / 2, (courtyard[1] + courtyard[3]) / 2] as [number, number], side, pads: [] });

function board(parts: ReturnType<typeof part>[], zones: unknown[] = []): BoardState {
  return { parts, routing: { tracks: [], vias: [], zones }, drawings: null, groups: [] } as unknown as BoardState;
}

const move = (refs: string[], dxUm: number, dyUm: number, over: Partial<MovePreview> = {}): MovePreview => ({ refs, dxUm, dyUm, kind: "pcb", ...over });

test("two courtyards conflict when they overlap, not when they only touch", () => {
  assert.equal(boxesOverlap([0, 0, 10, 10], [5, 5, 15, 15]), true);
  assert.equal(boxesOverlap([0, 0, 10, 10], [10, 0, 20, 10]), false, "touching edges");
  assert.equal(boxesOverlap([0, 0, 10, 10], [11, 0, 20, 10]), false);
});

test("a footprint carried onto another conflicts with it, and both are flagged (UpdateConflicts( view, true ))", () => {
  const b = board([part("R1", [0, 0, 1000, 1000]), part("R2", [3000, 0, 4000, 1000]), part("R3", [0, 5000, 1000, 6000])]);
  assert.deepEqual([...courtyardConflicts(b, move(["R1"], 0, 0)).parts], [], "nothing overlaps where it is");
  const hit = courtyardConflicts(b, move(["R1"], 2500, 0));
  assert.deepEqual([...hit.parts].sort(), ["R1", "R2"], "R1 now overlaps R2");
  assert.equal(courtyardConflicts(b, null).parts.size, 0, "no move, no conflict");
});

test("only the footprints that are not moving are tested against the ones that are", () => {
  const b = board([part("A", [0, 0, 1000, 1000]), part("B", [500, 0, 1500, 1000])]);
  // A and B overlap, but both are carried: they are not in conflict with each other.
  assert.equal(courtyardConflicts(b, move(["A", "B"], 5000, 5000)).parts.size, 0);
});

test("courtyards on opposite sides do not conflict (F.CrtYd against B.CrtYd)", () => {
  const b = board([part("TOP", [0, 0, 1000, 1000]), part("BOT", [0, 0, 1000, 1000], "bottom")]);
  assert.equal(courtyardConflicts(b, move(["TOP"], 100, 100)).parts.size, 0);
  const b2 = board([part("TOP", [0, 0, 1000, 1000]), part("TOP2", [0, 0, 1000, 1000], "top")]);
  assert.equal(courtyardConflicts(b2, move(["TOP"], 100, 100)).parts.size, 2);
});

test("a turned footprint conflicts by the box it turns into, about its own anchor (the painter's transform, for the footprint-only moves)", () => {
  // A 2000 x 400 courtyard centred on (1000, 200); a quarter turn makes it 400 x 2000, reaching up to y = -800 and down to y = 1200.
  const wide = part("W", [0, 0, 2000, 400]);
  const neighbour = part("N", [800, 900, 1200, 1100]);
  const b = board([wide, neighbour]);
  assert.equal(courtyardConflicts(b, { refs: ["W"], dxUm: 0, dyUm: 0, rotateQuarterTurns: 0 }).parts.size, 0);
  assert.deepEqual([...courtyardConflicts(b, { refs: ["W"], dxUm: 0, dyUm: 0, rotateQuarterTurns: 1 }).parts].sort(), ["N", "W"]);
  const turned = movedBox(wide, { refs: ["W"], dxUm: 0, dyUm: 0, rotateQuarterTurns: 1 })!;
  assert.deepEqual(turned.map(Math.round), [800, -800, 1200, 1200]);
});

test("a carry (kind pcb) moves the box through the carry matrix, turn, flip and move", () => {
  const p = part("C", [0, 0, 1000, 500]);
  assert.deepEqual(movedBox(p, move(["C"], 300, -200))!.map(Math.round), [300, -200, 1300, 300]);
  // Flipped about x = 0 (the flip pivot), then moved by 100.
  assert.deepEqual(movedBox(p, move(["C"], 100, 0, { flipped: true, flipPivotUm: [0, 0] }))!.map(Math.round), [-900, 0, 100, 500]);
});

test("Pack and Move's per-footprint shift is added for the footprint-only moves", () => {
  const p = part("P", [0, 0, 1000, 1000]);
  assert.deepEqual(movedBox(p, { refs: ["P"], dxUm: 10, dyUm: 20, perRefOffsetUm: { P: [5, 5] } })!.map(Math.round), [15, 25, 1015, 1025]);
});

test("a rule area that keeps footprints out conflicts with a footprint carried into it, and only then", () => {
  const keepout = { id: "KO", is_rule_area: true, keepout_footprints: true, outline: [[5000, 0], [8000, 0], [8000, 3000], [5000, 3000]] };
  const other = { id: "Z", is_rule_area: true, keepout_footprints: false, outline: [[0, 5000], [3000, 5000], [3000, 8000], [0, 8000]] };
  const b = board([part("R1", [0, 0, 1000, 1000])], [keepout, other]);
  assert.equal(courtyardConflicts(b, move(["R1"], 100, 100)).zones.size, 0);
  const into = courtyardConflicts(b, move(["R1"], 5500, 500));
  assert.deepEqual([...into.zones], ["KO"]);
  assert.deepEqual([...into.parts], ["R1"], "the footprint is flagged, the zone is not a footprint");
  // The other rule area does not keep footprints out.
  assert.equal(courtyardConflicts(b, move(["R1"], 500, 5500)).zones.size, 0);
});

test("a polygon and a box meet by a corner inside, by a box corner inside the polygon, or by edges crossing", () => {
  const tri: [number, number][] = [[0, 0], [100, 0], [0, 100]];
  assert.equal(polygonHitsBox(tri, [10, 10, 20, 20]), true, "box inside the polygon");
  assert.equal(polygonHitsBox(tri, [-50, -50, 1000, 1000]), true, "polygon inside the box");
  assert.equal(polygonHitsBox(tri, [90, 90, 200, 200]), false, "beyond the hypotenuse");
  assert.equal(polygonHitsBox([[0, 0], [100, 0], [100, 10], [0, 10]], [40, -50, 60, 50]), true, "a thin polygon crossing the box with no corner in either");
});
