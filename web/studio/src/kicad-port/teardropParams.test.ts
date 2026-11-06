import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { applyTeardropForm, DEFAULT_TEARDROP_PARAMS, padTeardrops, teardropForm, teardropFormChanged, viaTeardrops } from "./teardropParams";

const board = { routing: { teardrop_settings: { enabled: true, target_vias: true, target_pth_pads: false, target_smd_pads: true, best_length_ratio: 0.6, best_width_ratio: 0.9, max_len_nm: 1_500_000, max_width_nm: 2_000_000, width_to_size_filter_ratio: 0.8 } } } as unknown as BoardState;

test("an item without parameters of its own shows Board Setup's block for its kind; one with its own shows them", () => {
  const v = viaTeardrops(board, {});
  assert.deepEqual([v.enabled, v.best_length_ratio, v.max_length, v.filter_ratio], [true, 0.6, 1_500_000, 0.8]);
  assert.equal(padTeardrops(board, { kind: "through_hole" }).enabled, false, "PTH pads are not targeted");
  assert.equal(padTeardrops(board, { kind: "smd" }).enabled, true);
  const own = { ...DEFAULT_TEARDROP_PARAMS, best_length_ratio: 0.3 };
  assert.equal(viaTeardrops(board, { teardrops: own }), own);
});

test("the dialogs' Teardrops part: percentages, indeterminate where the items differ, only a changed field applied to each item", () => {
  const a = { ...DEFAULT_TEARDROP_PARAMS, enabled: true, best_length_ratio: 0.5 };
  const b = { ...DEFAULT_TEARDROP_PARAMS, enabled: false, best_length_ratio: 0.5, max_length: 3_000_000 };
  const before = teardropForm([a, b]);
  assert.deepEqual([before.enabled, before.lenPercent, before.maxLen, before.widthPercent], [null, 50, null, 100]);
  assert.equal(teardropFormChanged(before, before), false);
  const form = { ...before, lenPercent: 80 };
  assert.ok(teardropFormChanged(before, form));
  assert.deepEqual(applyTeardropForm(b, before, form), { ...b, best_length_ratio: 0.8 }, "the item keeps its own enable and maximum length");
});
