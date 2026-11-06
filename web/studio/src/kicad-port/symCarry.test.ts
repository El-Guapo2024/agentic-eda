// What the Symbol Editor carries under the cursor (symCarry.ts): the Pin tool's pin and the Text tool's text after their dialogs (`TwoClickPlace`), and the new items of Paste
// and Duplicate (the move they start from the cursor).
import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import { carriedItems, pastedText, PASTE_TEXT_LIMIT, rebased, snappedOrigin, translateGraphic, type SymCarry } from "./symCarry";

const pin: LibrarySymbolPin = { id: undefined, number: "1", name: "A", electrical_type: "input", shape: "line", at: { x: 2.54, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null };

test("a pin carried after Pin Properties is held by its own position: it is where the snapped cursor is (`item->SetPosition( cursorPos )`)", () => {
  const c: SymCarry = { kind: "pin", pins: [pin], graphics: [], originNm: [2_540_000, 0], deltaNm: [0, 0] };
  // the cursor at (5.08, -2.54) mm on screen: the symbol's +y up makes that (5.08, 2.54)
  const at = carriedItems({ ...c, deltaNm: [5_080_000 - 2_540_000, -2_540_000] }).pins[0]!.at;
  assert.deepEqual(at, { x: 5.08, y: 2.54 });
  assert.equal(carriedItems(c).pins[0], pin, "no delta: the items as they were");
});

test("Paste and Duplicate hold their items by the cursor when the action ran: they keep their place relative to it", () => {
  const rect: LibrarySymbolGraphic = { kind: "rectangle", unit: 1, body_style: 1, start: { x: -1, y: -1 }, end: { x: 1, y: 1 }, stroke_mm: 0.254, fill: "none" };
  const c: SymCarry = { kind: "paste", pins: [pin], graphics: [rect], originNm: snappedOrigin({ x: 10_100_000, y: -3_800_000 }, 1_270_000), deltaNm: [1_270_000, 1_270_000] };
  assert.deepEqual(c.originNm, [10_160_000, -3_810_000], "the cursor on the grid");
  const now = carriedItems(c);
  assert.deepEqual([now.pins[0]!.at, (now.graphics[0] as { start: unknown }).start], [{ x: 3.81, y: -1.27 }, { x: 0.27, y: -2.27 }]);
});

test("rebased: the items where they are now, held at the cursor, so a turn while carried turns them there", () => {
  const c: SymCarry = { kind: "duplicate", pins: [pin], graphics: [], originNm: [0, 0], deltaNm: [2_540_000, 0] };
  const r = rebased(c);
  assert.deepEqual([r.pins[0]!.at, r.originNm, r.deltaNm], [{ x: 5.08, y: 0 }, [2_540_000, 0], [0, 0]]);
});

test("translateGraphic moves every point of every kind", () => {
  const bez: LibrarySymbolGraphic = { kind: "bezier", unit: 0, body_style: 1, start: { x: 0, y: 0 }, c1: { x: 1, y: 1 }, c2: { x: 2, y: 1 }, end: { x: 3, y: 0 }, stroke_mm: 0, fill: "none" };
  assert.deepEqual(translateGraphic(bez, 1, -1), { ...bez, start: { x: 1, y: -1 }, c1: { x: 2, y: 0 }, c2: { x: 3, y: 0 }, end: { x: 4, y: -1 } });
  const t: LibrarySymbolGraphic = { kind: "text", unit: 0, body_style: 1, text: "x", at: { x: 0.1, y: 0.2 }, angle_deg: 0, size_mm: 1.27 };
  assert.deepEqual((translateGraphic(t, 0.2, 0.1) as { at: unknown }).at, { x: 0.3, y: 0.3 });
});

test("Paste of a clipboard that is not a symbol: a text at the origin, common to all units, cut at 5000 characters", () => {
  assert.deepEqual(pastedText("hello"), { kind: "text", unit: 0, body_style: 0, text: "hello", at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27 });
  const long = "x".repeat(PASTE_TEXT_LIMIT + 10);
  assert.equal((pastedText(long) as { text: string }).text, `${"x".repeat(PASTE_TEXT_LIMIT)}...`);
});
