import { test } from "node:test";
import assert from "node:assert/strict";
import type { MenusFile } from "../kicad/types";
import { SCH_STUDIO_MENU_ITEMS, withStudioItems } from "./studioMenuItems";

const file = (): MenusFile => ({
  meta: { generated: true, kicadCommit: null, kicadCommitDate: null, extractedAt: null, sourceFiles: [] },
  menus: [
    { label: "File", items: [{ type: "item", action: "common.Control.new" }] },
    { label: "Tools", items: [{ type: "item", action: "eeschema.EditorControl.annotate" }] },
  ],
});

test("the studio's items are appended to the menu they name and to no other", () => {
  const out = withStudioItems(file(), SCH_STUDIO_MENU_ITEMS);
  assert.equal(out.menus[0]!.items.length, 1);
  const tools = out.menus[1]!.items;
  assert.equal(tools.length, 3);
  assert.deepEqual(tools[0], { type: "item", action: "eeschema.EditorControl.annotate" });
  assert.equal(tools[1]!.type, "separator");
  const last = tools[2]!;
  assert.equal(last.type === "item" && last.action, "studio.Sheets.reorganize");
  assert.equal(last.type === "item" && last.label, "Reorganize into Module Sheets");
});

test("the extracted menus are not changed, and a menu that is not there is skipped", () => {
  const original = file();
  const out = withStudioItems(original, { Tools: [{ type: "separator" }], Nowhere: [{ type: "separator" }] });
  assert.equal(original.menus[1]!.items.length, 1);
  assert.equal(out.menus.length, 2);
});
