import { test } from "node:test";
import assert from "node:assert/strict";
import { symbolSnapItems } from "./symSnapItems";
import { SchGridHelper, type SchSnapItem } from "./schGridHelper";
import type { GridEnv } from "./gridHelperBase";
import type { LibrarySymbol, LibrarySymbolPin } from "../api/types";

const pin = (id: string, x: number, y: number, over: Partial<LibrarySymbolPin> = {}): LibrarySymbolPin =>
  ({ id, number: id, name: id, electrical_type: "passive", shape: "line", at: { x, y }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null, ...over }) as LibrarySymbolPin;
const sym = (pins: LibrarySymbolPin[], graphics: LibrarySymbol["graphics"] = []): LibrarySymbol => ({ lib_id: "L:S", pins, graphics }) as unknown as LibrarySymbol;
// A 50 mil grid (1270 um), 0.05 px per um: the snap range is 55 mil, the grid is coarser than the anchors' reach.
const env = (over: Partial<GridEnv> = {}): GridEnv => ({ scale: 0.05, gridUm: 1270, visibleGridUm: 1270, origin: [0, 0], gridSizeOf: () => 1270, ...over });
const near = (got: readonly number[], want: readonly number[], tol = 1e-6) => assert.ok(Math.abs(got[0]! - want[0]!) <= tol && Math.abs(got[1]! - want[1]!) <= tol, `expected (${want}) got (${got})`);

test("a pin is an item at its connection point in the editor's own space (um, y down)", () => {
  const items = symbolSnapItems(sym([pin("1", -5.08, 2.54), pin("2", 5.08, -2.54)]), 1, 1, false);
  assert.deepEqual(items.map((i) => [i.id, i.kind, i.position, i.connectable]), [["1", "pin", [-5080, -2540], true], ["2", "pin", [5080, 2540], true]]);
});

test("only the unit and body style being edited, hidden pins only when shown", () => {
  const s = sym([pin("a", 0, 0), pin("b", 1, 0, { unit: 2 }), pin("c", 2, 0, { unit: 0, body_style: 0 }), pin("d", 3, 0, { body_style: 2 }), pin("e", 4, 0, { hidden: true, body_style: 0 })]);
  assert.deepEqual(symbolSnapItems(s, 1, 1, false).map((i) => i.id), ["a", "c"], "c is shared by every unit and body style");
  assert.deepEqual(symbolSnapItems(s, 2, 1, false).map((i) => i.id), ["b", "c"]);
  assert.deepEqual(symbolSnapItems(s, 1, 2, true).map((i) => i.id), ["c", "d", "e"]);
  assert.deepEqual(symbolSnapItems(null, 1, 1, false), []);
});

test("a text is an item of its own; a shape gives none", () => {
  const items = symbolSnapItems(sym([], [{ kind: "text", id: "t", unit: 1, body_style: 1, text: "ABC", at: { x: 1.27, y: 0 }, angle_deg: 0, size_mm: 1.27 }, { kind: "rectangle", id: "r", unit: 1, body_style: 1, start: { x: 0, y: 0 }, end: { x: 1, y: 1 }, stroke_mm: 0.25, fill: "none" }]), 1, 1, false);
  assert.deepEqual(items.map((i) => [i.id, i.kind, i.connectable]), [["t", "text", false]]);
});

test("with the grid off the cursor goes to the end of a pin; on the grid it stays on the grid; Shift turns the anchors off", () => {
  const items: SchSnapItem[] = symbolSnapItems(sym([pin("1", 3.1, 0.7)]), 1, 1, false);
  const h = new SchGridHelper(env(), items);
  h.setUseGrid(false);
  near(h.bestSnapAnchor([3180, -640], "connectable", new Set()), [3100, -700]);
  const g = new SchGridHelper(env(), items);
  near(g.bestSnapAnchor([3180, -640], "connectable", new Set()), [3810, -1270 + 0], 1300); // a grid point, not the pin
  const s = new SchGridHelper(env(), items);
  s.setUseGrid(false);
  s.setSnap(false);
  near(s.bestSnapAnchor([3180, -640], "connectable", new Set()), [3180, -640]);
  // the pin being moved is not an anchor
  const k = new SchGridHelper(env(), items);
  k.setUseGrid(false);
  near(k.bestSnapAnchor([3180, -640], "connectable", new Set(["1"])), [3180, -640]);
});
