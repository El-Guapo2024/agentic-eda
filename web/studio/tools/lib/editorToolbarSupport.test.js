// Every action on the Footprint Editor's and the Symbol Editor's KiCad toolbars is triaged in src/kicad/editor_toolbar_support.json: "supported", or the
// reason it is not ported. A button is drawn for every KiCad toolbar item (with KiCad's icon, tools/lib/toolbarIcons.test.js); this keeps "drawn" honest --
// a new item in a regenerated fp_toolbars.json / sym_toolbars.json has to be decided on, and a stale entry cannot linger.
//
// Plain Node, no TypeScript involved -- run directly:
//   node --test tools/lib/editorToolbarSupport.test.js
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const readJson = (rel) => JSON.parse(readFileSync(join(ROOT, rel), "utf8"));

const support = readJson("src/kicad/editor_toolbar_support.json");
const actions = new Set(readJson("src/kicad/actions.json").actions.map((a) => a.name));

/** Every action a toolbar file names, buttons and the entries of its dropdown groups. */
function toolbarActions(rel) {
  const names = new Set();
  for (const bar of readJson(rel).toolbars) {
    for (const item of bar.items) {
      if (item.type === "action") names.add(item.action);
      else if (item.type === "group") for (const m of item.items) names.add(m);
    }
  }
  return names;
}

for (const [editor, file] of [
  ["footprint", "src/kicad/fp_toolbars.json"],
  ["symbol", "src/kicad/sym_toolbars.json"],
]) {
  test(`${editor} editor: every toolbar action is triaged, and nothing stale is listed`, () => {
    const named = toolbarActions(file);
    const listed = new Set(Object.keys(support[editor]));
    const untriaged = [...named].filter((n) => !listed.has(n));
    const stale = [...listed].filter((n) => !named.has(n));
    assert.deepEqual(untriaged, [], `add these to editor_toolbar_support.json (${editor}): "supported" or the reason they are not ported`);
    assert.deepEqual(stale, [], `no longer on the ${editor} toolbar: remove from editor_toolbar_support.json`);
  });

  test(`${editor} editor: each entry is "supported" or a reason, and names a real KiCad action`, () => {
    for (const [name, state] of Object.entries(support[editor])) {
      assert.ok(actions.has(name), `${name} is not in actions.json`);
      assert.ok(state === "supported" || /^not ported: .{5,}/.test(state), `${name}: "${state}" -- say "supported" or "not ported: <the reason>"`);
    }
  });
}

test("the two editors support a real set of their toolbars (the table is not all reasons)", () => {
  for (const editor of ["footprint", "symbol"]) {
    const states = Object.values(support[editor]);
    const supported = states.filter((s) => s === "supported").length;
    assert.ok(supported >= 20, `${editor}: only ${supported} supported actions`);
  }
});
