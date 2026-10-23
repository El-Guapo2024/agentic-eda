import { test } from "node:test";
import assert from "node:assert/strict";
import { popupMenuKey } from "./popupMenuKeys";

test("popupMenuKey: Escape dismisses an open popup menu and goes no further (the dialog under it and the tool stay); other keys are not the menu's", () => {
  assert.deepEqual(popupMenuKey("Escape"), { close: true, consume: true });
  assert.deepEqual(popupMenuKey("a"), { close: false, consume: false });
  assert.deepEqual(popupMenuKey("Enter"), { close: false, consume: false });
});
