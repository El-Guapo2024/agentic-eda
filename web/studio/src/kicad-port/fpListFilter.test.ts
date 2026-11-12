import { test } from "node:test";
import assert from "node:assert/strict";
import { listFilter } from "./fpListFilter";

test("listFilter (EDA_LIST_DIALOG::textChangeInFilterBox): *text* without case, * and ? wildcards, other characters literal", () => {
  assert.equal(listFilter("")("R1"), true);
  assert.equal(listFilter("r1")("R10"), true);
  assert.equal(listFilter("U?")("U3"), true);
  assert.equal(listFilter("U?")("U"), false);
  assert.equal(listFilter("C*2")("C12"), true);
  assert.equal(listFilter("R.")("R1"), false);
  assert.equal(listFilter("(")("J(1)"), true);
});
