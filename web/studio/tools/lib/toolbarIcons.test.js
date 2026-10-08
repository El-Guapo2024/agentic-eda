// Every toolbar button of every editor tab must have the icon KiCad gives its action -- no blank grey square (the `.icon-placeholder` Toolbar.tsx draws when
// an action resolves to no icon). The toolbars are data (src/kicad/*toolbars.json, written by tools/extract-toolbars.js, extract-sch-toolbars.js and
// extract-editor-toolbars.js), and each action's icon is the `BITMAPS::` entry of its TOOL_ACTION (actions.json; the 3D viewer's own actions carry theirs in
// viewer3d_toolbars.json). An icon counts as present when icons.json maps that BITMAPS name to a file and the file exists for BOTH themes
// (public/icons/{light,dark}/, KiCad's resources/bitmaps_png/sources, CC-BY-SA).
//
// Plain Node, no TypeScript involved -- run directly:
//   node --test tools/lib/toolbarIcons.test.js
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const readJson = (rel) => JSON.parse(readFileSync(join(ROOT, rel), "utf8"));

const actions = new Map(readJson("src/kicad/actions.json").actions.map((a) => [a.name, a]));
const icons = readJson("src/kicad/icons.json").icons;

/** The toolbar files, with where each tab's toolbars live: [file, who shows it]. */
const TOOLBAR_FILES = [
  ["src/kicad/toolbars.json", "PCB editor"],
  ["src/kicad/sch_toolbars.json", "Schematic editor"],
  ["src/kicad/fp_toolbars.json", "Footprint editor"],
  ["src/kicad/sym_toolbars.json", "Symbol editor"],
  ["src/kicad/viewer3d_toolbars.json", "3D viewer"],
];

/** Why `iconName` is not a drawable icon, or null when it is. */
function iconProblem(iconName) {
  if (!iconName || iconName === "INVALID_BITMAP") return "the action has no icon (BITMAPS::INVALID_BITMAP / none)";
  const file = icons[iconName];
  if (!file) return `icons.json has no entry for BITMAPS::${iconName} (run tools/extract-icons.js)`;
  for (const theme of ["light", "dark"]) {
    if (!existsSync(join(ROOT, "public", "icons", theme, file))) return `public/icons/${theme}/${file} is missing`;
  }
  return null;
}

test("every toolbar button on every tab has an icon (both themes)", () => {
  const problems = [];
  let buttons = 0;
  for (const [rel, who] of TOOLBAR_FILES) {
    const file = readJson(rel);
    // The 3D viewer's actions are not in actions.json: its toolbar file carries them.
    const own = new Map([...(file.actions ?? []), ...(file.otherActions ?? [])].map((a) => [a.name, a]));
    const iconOf = (name) => (own.get(name) ?? actions.get(name))?.icon ?? null;
    const known = (name) => own.has(name) || actions.has(name);
    for (const bar of file.toolbars) {
      for (const item of bar.items) {
        if (item.type === "action") {
          buttons++;
          const where = `${who} ${bar.id} toolbar: ${item.action}`;
          if (!known(item.action)) problems.push(`${where}: no such action`);
          else {
            const p = iconProblem(iconOf(item.action));
            if (p) problems.push(`${where}: ${p}`);
          }
        } else if (item.type === "group") {
          buttons++;
          const where = `${who} ${bar.id} toolbar: group "${item.label}"`;
          const p = iconProblem(item.icon);
          if (p) problems.push(`${where}: ${p}`);
          // The group's entries are drawn in its dropdown with their own icons.
          for (const member of item.items) {
            const mp = iconProblem(iconOf(member));
            if (mp) problems.push(`${where}, entry ${member}: ${mp}`);
          }
        }
      }
    }
  }
  assert.ok(buttons > 150, `expected the toolbars to hold a few hundred buttons, found ${buttons} (a toolbar file lost its items?)`);
  assert.deepEqual(problems, [], `toolbar buttons without an icon:\n${problems.join("\n")}`);
});

test("the extra icons the 3D viewer's appearance panel and view presets use exist too", () => {
  const file = readJson("src/kicad/viewer3d_toolbars.json");
  const problems = [];
  for (const a of file.otherActions ?? []) {
    const p = iconProblem(a.icon);
    if (p) problems.push(`${a.name}: ${p}`);
  }
  assert.deepEqual(problems, [], problems.join("\n"));
  // The six face views keep their buttons (KiCad reaches them from its View menu and hotkeys; this app also puts them in the appearance panel).
  const names = new Set((file.otherActions ?? []).map((a) => a.name));
  for (const v of ["viewFront", "viewBack", "viewLeft", "viewRight", "viewTop", "viewBottom"]) assert.ok(names.has(`3DViewer.Control.${v}`), `3DViewer.Control.${v} is not in viewer3d_toolbars.json`);
});

test("a toolbar button without an icon is caught (the check itself works)", () => {
  assert.ok(iconProblem(null));
  assert.ok(iconProblem("INVALID_BITMAP"));
  assert.ok(iconProblem("no_such_bitmap_name"));
  assert.equal(iconProblem("copy"), null);
});
