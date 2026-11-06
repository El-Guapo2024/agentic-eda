import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, FpTextInfo, Part } from "../api/types";
import { DEFAULT_SELECTION_FILTER, pickSelectionCandidates } from "../components/canvas/selectionCandidates";
import { boardDeleteCmds } from "./deleteCmds";
import { editableSelection, isLocked } from "./pcbTransform";
import { fpTextById, itemKind } from "./pcbItems";

const text = (id: string, x: number, y: number, s = "note"): FpTextInfo => ({ id, text: s, x, y, angle: 0, w: 1_000_000, h: 1_000_000, thickness: 150_000, layer: "F.SilkS", halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: true, knockout: false });
const part = (ref: string, texts: FpTextInfo[]): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [4_000_000, 2_000_000], at: [0, 0], rot: 0, side: "top", label: "above", courtyard: [-2_000_000, -1_000_000, 2_000_000, 1_000_000], pads: [], texts }) as Part;
const board = (parts: Part[], extra: Partial<BoardState> = {}): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts, rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...extra }) as BoardState;

test("UI-LEFT P15: a footprint's own text is an item of the board -- picked by a click over the footprint, locked with it, carried with it, deleted on its own", () => {
  const b = board([part("U1", [text("U1~t0", 0, 0), text("U1~t1", 5_000_000, 0, "far")])]);
  assert.equal(itemKind(b, "U1~t0"), "fptext");
  assert.equal(fpTextById(b, "U1~t1")?.text.text, "far");
  // `GuessSelectionCandidates`: the text is smaller than the footprint it sits in
  const hit = pickSelectionCandidates(b, 0, 0, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, null, false, new Set(), false, false);
  assert.deepEqual(hit.map((c) => [c.kind, c.id]), [["fptext", "U1~t0"]]);
  // the Text filter keeps it out (`itemPassesFilter`: a `PCB_TEXT` is text)
  const none = pickSelectionCandidates(b, 5_000_000, 0, 150_000, 10_000, { ...DEFAULT_SELECTION_FILTER, text: false }, {}, null, false, new Set(), false, false);
  assert.deepEqual(none.map((c) => c.kind), []);
  // a locked footprint's text is locked (`PCB_TEXT::IsLocked`)
  assert.equal(isLocked(board(b.parts, { locked: ["U1"] }), "U1~t0"), true);
  // with its footprint in the selection it goes with the footprint (`FilterCollectorForHierarchy`)
  assert.deepEqual(editableSelection(b, ["U1~t0", "U1"]).ids, ["U1"]);
  assert.deepEqual(editableSelection(b, ["U1~t0"]).ids, ["U1~t0"]);
  // deleted from its footprint, the highest index first
  assert.deepEqual(boardDeleteCmds(b, ["U1~t0", "U1~t1"]).cmds, [
    { op: "edit_board_footprint_text", id: "U1~t1", delete: true },
    { op: "edit_board_footprint_text", id: "U1~t0", delete: true },
  ]);
});
