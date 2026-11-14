import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveTemplates, transferGlobalTemplates } from "./fieldNameTemplates";

const t = (name: string, visible = false) => ({ name, visible, url: false });

test("resolveTemplates: the project's first, then the global ones it does not name", () => {
  assert.deepEqual(resolveTemplates([t("MPN", true)], [t("MPN"), t("Supplier")]), [t("MPN", true), t("Supplier")]);
  assert.deepEqual(resolveTemplates([], [t("A")]), [t("A")]);
});

test("TransferDataFromWindow: white space at an end is asked about, a mandatory or empty name dropped, a repeated name replaces the first", async () => {
  const asked: string[] = [];
  const kept = await transferGlobalTemplates([t(" MPN "), t("Value"), t(""), t("Lot"), t("Lot", true)], async (m) => (asked.push(m), true));
  assert.deepEqual(asked, ["The field name ' MPN ' contains trailing and/or leading white space."]);
  assert.deepEqual(kept, [t("MPN"), t("Lot", true)]);
  const keepSpace = await transferGlobalTemplates([t("X ")], async () => false);
  assert.deepEqual(keepSpace, [t("X ")], "Keep White Space");
});
