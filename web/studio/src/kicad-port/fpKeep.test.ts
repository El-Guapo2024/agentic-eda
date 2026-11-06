import { test } from "node:test";
import assert from "node:assert/strict";
import { fillChoiceOf, fillOfChoice, isHiddenText, isLockedItem, shapeLookOf, textLookOf } from "./fpKeep";

const fp = {
  kicad: {
    stroke_types: { g1: "dash" },
    hatches: { g1: "cross_hatch" },
    text_extras: [{ id: "t1", bold: true, valign: "top", width_nm: 1_200_000 }],
    unlocked_texts: ["t1"],
    hidden_texts: ["t2"],
    locked: ["g1"],
  },
};

test("the looks a library footprint keeps by id are read for the dialogs and the painter", () => {
  assert.deepEqual(shapeLookOf(fp, "g1"), { line_style: "dash", hatch: "cross_hatch" });
  assert.deepEqual(shapeLookOf(fp, "g2"), { line_style: "solid", hatch: null });
  assert.deepEqual(textLookOf(fp, "t1"), { bold: true, italic: false, valign: -1, width_nm: 1_200_000, knockout: false, keep_upright: false });
  assert.deepEqual(textLookOf(fp, "t9"), { bold: false, italic: false, valign: 0, width_nm: null, knockout: false, keep_upright: true });
  assert.ok(isHiddenText(fp, "t2") && !isHiddenText(fp, "t1") && !isHiddenText(fp, undefined));
  assert.ok(isLockedItem(fp, "g1") && !isLockedItem(null, "g1"));
});

test("the shape dialog's Fill is the filled flag and the hatch", () => {
  assert.equal(fillChoiceOf(true, null), "solid");
  assert.equal(fillChoiceOf(false, "hatch"), "hatch");
  assert.equal(fillChoiceOf(false, null), "none");
  assert.deepEqual(fillOfChoice("reverse_hatch"), { filled: false, hatch: "reverse_hatch" });
  assert.deepEqual(fillOfChoice("solid"), { filled: true, hatch: null });
});
