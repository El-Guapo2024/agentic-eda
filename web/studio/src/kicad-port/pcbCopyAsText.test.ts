import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { boardItemAsText } from "./pcbCopyAsText";

test("Copy as Text: a text's shown text, a table's cells tab and newline separated, a field and a text box (copyToClipboardAsText)", () => {
  const cell = (row: number, col: number, text: string, shown?: string) => ({ row, col, text, ...(shown ? { shown } : {}) });
  const board = {
    parts: [{ ref: "R1", placed: true, fields: [{ id: "R1:Value", name: "Value", text: "10k" }] }],
    drawings: {
      shapes: [],
      texts: [{ id: "t1", content: "${REVISION}", shown: "B" }],
      dimensions: [{ id: "d1", text: "12.3" }],
      groups: [],
      text_boxes: [{ id: "b1", text: "box", shown: "box" }],
      tables: [{ id: "tb", rows: 2, cols: 2, cells: [cell(0, 0, "a"), cell(0, 1, "b"), cell(1, 0, "c"), cell(1, 1, "${X}", "x")] }],
    },
  } as unknown as BoardState;
  assert.equal(boardItemAsText(board, "t1"), "B", "the shown text, variables resolved");
  assert.equal(boardItemAsText(board, "tb"), "a\tb\nc\tx");
  assert.equal(boardItemAsText(board, "d1"), "12.3");
  assert.equal(boardItemAsText(board, "R1:Value"), "10k");
  assert.equal(boardItemAsText(board, "b1"), "box");
  assert.equal(boardItemAsText(board, "nope"), undefined);
});
