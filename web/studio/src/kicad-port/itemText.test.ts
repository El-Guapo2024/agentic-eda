import { test } from "node:test";
import assert from "node:assert/strict";
import { selectionAsText, datasheetTarget } from "./itemText";

test("selectionAsText trims, drops empties, joins with newline", () => {
  assert.equal(selectionAsText(["  hello ", "", null, undefined, "world\n"]), "hello\nworld");
  assert.equal(selectionAsText([]), "");
});

test("datasheetTarget: empty and '~' mean none; http(s)/file launch; www gets https; else unresolvable", () => {
  assert.deepEqual(datasheetTarget(""), { kind: "none" });
  assert.deepEqual(datasheetTarget(" ~ "), { kind: "none" });
  assert.deepEqual(datasheetTarget(null), { kind: "none" });
  assert.deepEqual(datasheetTarget("https://a.b/c.pdf"), { kind: "url", url: "https://a.b/c.pdf" });
  assert.deepEqual(datasheetTarget("www.ti.com/x"), { kind: "url", url: "https://www.ti.com/x" });
  assert.deepEqual(datasheetTarget("ds/part.pdf"), { kind: "unresolvable", text: "ds/part.pdf" });
});
