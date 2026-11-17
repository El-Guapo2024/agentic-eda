import { test } from "node:test";
import assert from "node:assert/strict";
import { OPEN_RECENT_ACTION, projectCaption, projectNameProblem, withWorkspaceFileMenu } from "./workspace";
import type { MenusFile } from "../kicad/types";
import { boardFitBounds } from "./view";

test("projectNameProblem: a project is a folder of the workspace, named, never a path (the server's check_name, word for word)", () => {
  for (const bad of ["", "  ", "a/b", "a\\b", "..", "../x", "x..y", ".hidden", "tab\there", " lead"]) assert.notEqual(projectNameProblem(bad), null, JSON.stringify(bad));
  for (const good of ["mcu30", "safety-power.old", "My Board 2"]) assert.equal(projectNameProblem(good), null, good);
  assert.equal(projectNameProblem("../x"), "Project name '../x' must not contain path separators or '..'.");
  assert.equal(projectNameProblem(""), "Please enter a project name.");
});

test("projectCaption: '<project> — <editor>' as KiCad's caption, '[no project loaded]' with none", () => {
  assert.equal(projectCaption("mcu30", "pcb"), "mcu30 — PCB Editor");
  assert.equal(projectCaption("mcu30", "schematic"), "mcu30 — Schematic Editor");
  assert.equal(projectCaption(null, "pcb"), "[no project loaded] — EDA Studio");
});

test("withWorkspaceFileMenu: New... and Open... become the project manager's, Open Recent follows Open, the rest is KiCad's", () => {
  const file: MenusFile = {
    meta: {} as MenusFile["meta"],
    menus: [
      { label: "File", items: [{ type: "item", action: "common.Control.new" }, { type: "item", action: "common.Control.open" }, { type: "separator" }, { type: "item", action: "common.Control.save" }] },
      { label: "Edit", items: [{ type: "item", action: "common.Control.new" }] },
    ],
  };
  const out = withWorkspaceFileMenu(file, ["b", "a"]);
  const items = out.menus[0]!.items;
  assert.deepEqual(items[0], { type: "item", action: "common.Control.new", label: "New Project..." });
  assert.deepEqual(items[1], { type: "item", action: "common.Control.open", label: "Open Project..." });
  assert.deepEqual(items[2], {
    type: "submenu",
    label: "Open Recent",
    items: [
      { type: "item", action: OPEN_RECENT_ACTION, arg: "b", label: "b" },
      { type: "item", action: OPEN_RECENT_ACTION, arg: "a", label: "a" },
    ],
  });
  assert.deepEqual(items.slice(3), file.menus[0]!.items.slice(2));
  assert.deepEqual(out.menus[1], file.menus[1], "only the File menu");
  const empty = withWorkspaceFileMenu(file, []).menus[0]!.items[2];
  assert.ok(empty?.type === "submenu" && empty.items.length === 1 && empty.items[0]!.type === "item" && empty.items[0]!.disabled === true, "an empty history is a disabled entry");
});

test("boardFitBounds: the outline's box, else the page (PCB_BASE_FRAME::GetBoardBoundingBox on an empty board)", () => {
  assert.deepEqual(boardFitBounds([[1, 2], [5, 9]], [10, 20]), { minX: 1, minY: 2, maxX: 5, maxY: 9 });
  assert.deepEqual(boardFitBounds([], [420_000_000, 297_000_000]), { minX: 0, minY: 0, maxX: 420_000_000, maxY: 297_000_000 });
  assert.deepEqual(boardFitBounds(null, null), { minX: 0, minY: 0, maxX: 297_000_000, maxY: 210_000_000 });
});
