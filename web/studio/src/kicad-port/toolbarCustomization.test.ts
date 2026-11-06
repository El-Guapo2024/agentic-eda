// Customised toolbars (toolbarCustomization.ts): KiCad's toolbar JSON, which toolbar a frame shows, the actions list, and the page's tree edits
// (PANEL_TOOLBAR_CUSTOMIZATION at KiCad 8303b2ad).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  TOOLBAR_CONTROLS,
  actionEntries,
  addEntry,
  configFromJson,
  configOfStudioItems,
  controlsFor,
  dedupeControls,
  defaultShadowToolbars,
  deleteSelected,
  filterActionEntries,
  findNode,
  insertItem,
  isActionSupported,
  itemFromJson,
  itemToJson,
  moveSelected,
  parseToolbarSettings,
  parseToolbarTree,
  populateToolbarTree,
  renameGroup,
  serializeToolbarSettings,
  shadowToolbars,
  studioItemsOfConfig,
  toolbarChoices,
  toolbarConfig,
  toolbarsFromJson,
  toolbarsToJson,
  treeLabel,
  type TbConfig,
  type ToolbarAction,
  type ToolbarTree,
} from "./toolbarCustomization";
import type { ToolbarItem, ToolbarsFile } from "../kicad/types";

const actions = (JSON.parse(readFileSync("src/kicad/actions.json", "utf8")) as { actions: ToolbarAction[] }).actions;
const pcbFile = JSON.parse(readFileSync("src/kicad/toolbars.json", "utf8")) as ToolbarsFile;
const fpFile = JSON.parse(readFileSync("src/kicad/fp_toolbars.json", "utf8")) as ToolbarsFile;
const symFile = JSON.parse(readFileSync("src/kicad/sym_toolbars.json", "utf8")) as ToolbarsFile;
const v3d = JSON.parse(readFileSync("src/kicad/viewer3d_toolbars.json", "utf8")) as ToolbarsFile & { actions: ToolbarAction[]; otherActions: ToolbarAction[] };
const actionNames = new Set(actions.map((a) => a.name));
const has = (n: string) => actionNames.has(n);

test("the JSON of a toolbar item is KiCad's (to_json / from_json)", () => {
  const items: TbConfig = [
    { type: "TOOL", name: "common.Control.zoomRedraw" },
    { type: "SEPARATOR" },
    { type: "SPACER", size: 7 },
    { type: "CONTROL", name: "control.GridSelector" },
    { type: "TB_GROUP", groupName: "Units", items: ["common.Control.metricUnits", "common.Control.mils"] },
  ];
  const json = items.map(itemToJson);
  assert.deepEqual(json[2], { type: "SPACER", size: 7 });
  assert.deepEqual(json[4], { type: "TB_GROUP", group_name: "Units", group_items: [{ type: "TOOL", name: "common.Control.metricUnits" }, { type: "TOOL", name: "common.Control.mils" }] });
  assert.deepEqual(configFromJson(JSON.parse(JSON.stringify(json))), items);
  // the type is read without regard to case; an unknown type is a tool (TOOLBAR_ITEM's default); an empty item is a nameless tool, never shown
  assert.deepEqual(itemFromJson({ type: "separator" }), { type: "SEPARATOR" });
  assert.deepEqual(itemFromJson({ type: "WIDGET", name: "common.Control.zoomRedraw" }), { type: "TOOL", name: "common.Control.zoomRedraw" });
  assert.equal(itemFromJson({}), null);
  // a group keeps only its tools ("Unsupported group item type")
  assert.deepEqual(itemFromJson({ type: "TB_GROUP", group_name: "G", group_items: [{ type: "SEPARATOR" }, { type: "TOOL", name: "x.y.z" }] }), { type: "TB_GROUP", groupName: "G", items: ["x.y.z"] });
  assert.deepEqual(configFromJson({ not: "an array" }), []);
});

test("the toolbars param: by location, the first of a location kept, unknown names skipped; the store's text round-trips", () => {
  const stored = { TOP_MAIN: [{ type: "TOOL" as const, name: "a.b.c" }], LEFT: [{ type: "SEPARATOR" as const }] };
  const json = toolbarsToJson(stored);
  assert.deepEqual(json, [{ name: "LEFT", contents: [{ type: "SEPARATOR" }] }, { name: "TOP_MAIN", contents: [{ type: "TOOL", name: "a.b.c" }] }]);
  assert.deepEqual(toolbarsFromJson([{ name: "top_main", contents: [] }, { name: "TOP_MAIN", contents: [{ type: "SEPARATOR" }] }, { name: "BOTTOM", contents: [] }, 3]), { TOP_MAIN: [] });
  const text = serializeToolbarSettings({ pcb: { custom: true, toolbars: stored }, footprint: { custom: false, toolbars: {} } });
  const back = parseToolbarSettings(text);
  assert.deepEqual(back, { pcb: { custom: true, toolbars: stored }, footprint: { custom: false, toolbars: {} } });
  assert.ok(text.includes('"pcbnew-toolbars"') && text.includes('"custom_toolbars":true'), text);
  assert.deepEqual(parseToolbarSettings("{bad"), {});
  assert.deepEqual(parseToolbarSettings(null), {});
  assert.deepEqual(parseToolbarSettings('{"pcbnew-toolbars": 5}'), {});
});

test("GetToolbarConfig: nothing where the frame has no default; the stored one only with custom toolbars on", () => {
  const def = { TOP_MAIN: [{ type: "SEPARATOR" as const }] };
  const stored = { TOP_MAIN: [{ type: "TOOL" as const, name: "a.b.c" }], TOP_AUX: [{ type: "SEPARATOR" as const }] };
  assert.deepEqual(toolbarConfig(def, stored, "TOP_MAIN", true), stored.TOP_MAIN);
  assert.deepEqual(toolbarConfig(def, stored, "TOP_MAIN", false), def.TOP_MAIN);
  assert.equal(toolbarConfig(def, stored, "TOP_AUX", true), null);
  assert.deepEqual(toolbarChoices({ TOP_AUX: [], LEFT: [], TOP_MAIN: [] }), ["LEFT", "TOP_MAIN", "TOP_AUX"]);
  assert.deepEqual(shadowToolbars(def, stored), { TOP_MAIN: stored.TOP_MAIN });
  assert.deepEqual(defaultShadowToolbars(def), def);
});

test("isActionSupported and the actions list: the frame's kiface and common, icons only, no hidden ones, sorted without case, the controls with them", () => {
  assert.ok(isActionSupported("pcb", "pcbnew.InteractiveRouter.SingleTrack") && isActionSupported("footprint", "common.Control.zoomRedraw"));
  assert.ok(!isActionSupported("pcb", "eeschema.InteractiveDrawing.drawWire") && isActionSupported("symbol", "eeschema.InteractiveDrawing.drawWire"));
  assert.ok(isActionSupported("3d", "3DViewer.Control.flipView") && isActionSupported("3d", "common.Control.zoomInCenter") && !isActionSupported("3d", "common.Control.zoomIn"));
  const list = actionEntries("pcb", actions);
  assert.ok(list.every((e) => e.kind === "control" || (e.icon && (e.name.startsWith("common.") || e.name.startsWith("pcbnew.")))));
  assert.ok(!list.some((e) => e.name === "common.Control.cursorUp"), "a TOOLBAR_STATE::HIDDEN action is not offered");
  for (let i = 1; i < list.length; i++) assert.ok(list[i - 1]!.label.toLowerCase() <= list[i]!.label.toLowerCase(), `${list[i - 1]!.label} before ${list[i]!.label}`);
  const controls = list.filter((e) => e.kind === "control").map((e) => e.label).sort();
  assert.deepEqual(controls, ["Current variant", "Grid selector", "IPC/Scripting plugins", "Layer selector", "Override locks", "Track width selector", "Via diameter selector", "Zoom selector"]);
  assert.deepEqual(controlsFor("symbol").map((c) => c.uiName), ["Grid selector", "Zoom selector", "Symbol unit selector", "Symbol body style selector"]);
  assert.deepEqual(controlsFor("3d").map((c) => c.name), ["control.ZoomSelector"]);
  // the filter: the text and the tooltip, upper-cased
  const route = filterActionEntries(list, "route single");
  assert.ok(route.some((e) => e.name === "pcbnew.InteractiveRouter.SingleTrack"), route.map((e) => e.name).join());
  assert.ok(filterActionEntries(list, "Grid Selection box").some((e) => e.kind === "control" && e.name === "control.GridSelector"));
  assert.equal(filterActionEntries(list, "").length, list.length);
  // the 3D viewer: its own actions and the four zooms
  const list3d = actionEntries("3d", [...v3d.actions, ...v3d.otherActions, ...actions]);
  assert.ok(list3d.some((e) => e.name === "common.Control.zoomFitScreen") && list3d.some((e) => e.name === "3DViewer.Control.rotateXclockwise"));
  assert.ok(list3d.filter((e) => e.kind === "tool").every((e) => e.name.startsWith("3DViewer.") || e.name.startsWith("common.Control.zoom")));
});

test("the studio's default toolbars as KiCad's configuration and back draw the same", () => {
  const iconOf = (n: string) => actions.find((a) => a.name === n)?.icon ?? null;
  for (const [frame, file] of [["pcb", pcbFile], ["footprint", fpFile], ["symbol", symFile]] as const) {
    for (const tb of file.toolbars) {
      const cfg = configOfStudioItems(frame, tb.items);
      const labels = new Map(tb.items.flatMap((i) => (i.type === "control" ? [[i.control, i.label ?? i.control] as const] : [])));
      const back = studioItemsOfConfig(frame, cfg, { hasTool: has, iconOf, controlLabel: (id) => labels.get(id) });
      const want = tb.items.map((i): ToolbarItem => (i.type === "group" ? { ...i, icon: iconOf(i.items[0]!) } : i));
      assert.deepEqual(back, want, `${frame} ${tb.id}`);
    }
  }
  // the board's controls by KiCad's names
  const aux = configOfStudioItems("pcb", pcbFile.toolbars.find((t) => t.id === "auxiliary")!.items);
  assert.deepEqual(aux.filter((i) => i.type === "CONTROL").map((i) => (i as { name: string }).name), ["control.PCBTrackWidth", "control.PCBViaDia", "control.LayerSelector", "control.GridSelector", "control.ZoomSelector", "control.OverrideLocks"]);
  // what the frame lacks is left out: an unknown tool, a group of unknown tools, another frame's control
  const out = studioItemsOfConfig("footprint", [{ type: "TOOL", name: "nope.x.y" }, { type: "TB_GROUP", groupName: "G", items: ["nope.a.b"] }, { type: "CONTROL", name: "control.PCBTrackWidth" }, { type: "SPACER", size: 9 }], { hasTool: has, iconOf });
  assert.deepEqual(out, [{ type: "spacer", size: 9 }]);
  assert.equal(TOOLBAR_CONTROLS.filter((c) => c.id === "currentVariant").length, 2);
});

const ctx = { frame: "pcb" as const, hasTool: has };
const sample: TbConfig = [
  { type: "TOOL", name: "common.Control.zoomRedraw" },
  { type: "TB_GROUP", groupName: "Units", items: ["common.Control.metricUnits", "nope.x.y", "common.Control.mils"] },
  { type: "TOOL", name: "eeschema.InteractiveDrawing.drawWire" },
  { type: "TB_GROUP", groupName: "Empty", items: ["nope.x.y"] },
  { type: "CONTROL", name: "control.GridSelector" },
  { type: "CONTROL", name: "control.UnitSelector" },
  { type: "SPACER", size: 3 },
  { type: "SEPARATOR" },
];

test("populateToolbarTree / parseToolbarTree: unsupported and unknown rows left out, an empty group deleted, the first row selected", () => {
  const tree = populateToolbarTree(sample, ctx);
  assert.deepEqual(
    tree.nodes.map((n) => treeLabel(n, "pcb", (name) => actions.find((a) => a.name === name)?.label ?? name)),
    [actions.find((a) => a.name === "common.Control.zoomRedraw")!.label, "Units", "Grid selector", "Spacer: 3", "Separator"],
  );
  assert.equal(tree.selected, tree.nodes[0]!.id);
  assert.deepEqual(parseToolbarTree(tree), [
    { type: "TOOL", name: "common.Control.zoomRedraw" },
    { type: "TB_GROUP", groupName: "Units", items: ["common.Control.metricUnits", "common.Control.mils"] },
    { type: "CONTROL", name: "control.GridSelector" },
    { type: "SPACER", size: 3 },
    { type: "SEPARATOR" },
  ]);
});

const select = (t: ToolbarTree, id: number | null): ToolbarTree => ({ ...t, selected: id });

test("Insert Separator / Spacer / Group: after the selection, never into a group, the new row selected", () => {
  const tree = populateToolbarTree(sample, ctx);
  const group = tree.nodes[1]!;
  const s1 = insertItem(select(tree, tree.nodes[0]!.id), "separator");
  assert.equal(s1.nodes[1]!.kind, "separator");
  assert.equal(s1.selected, s1.nodes[1]!.id);
  const sp = insertItem(select(tree, null), "spacer");
  assert.deepEqual(sp.nodes[sp.nodes.length - 1], { id: tree.nextId, kind: "spacer", size: 5 });
  const g = insertItem(select(tree, group.id), "group");
  assert.deepEqual({ ...g.nodes[2]!, id: 0 }, { id: 0, kind: "group", name: "Group", children: [] });
  // a group's tool selected: nothing happens ("Can't add a group onto a group", "Can't insert a separator in a group yet")
  const child = (group as { children: Array<{ id: number }> }).children[0]!.id;
  for (const kind of ["separator", "spacer", "group"] as const) assert.deepEqual(insertItem(select(tree, child), kind), select(tree, child));
  // a new empty group is not stored (populate drops it the next time) but parse keeps it as KiCad does
  assert.deepEqual(parseToolbarTree(g)[2], { type: "TB_GROUP", groupName: "Group", items: [] });
});

test("Add: into a selected group at its end, after a selected row among its siblings, at the end with nothing selected; a control moves off the other toolbars", () => {
  const tree = populateToolbarTree(sample, ctx);
  const group = tree.nodes[1] as Extract<ToolbarTree["nodes"][number], { kind: "group" }>;
  const shadow = { TOP_MAIN: sample, TOP_AUX: [{ type: "CONTROL" as const, name: "control.ZoomSelector" }, { type: "SEPARATOR" as const }] };
  const a = addEntry(select(tree, group.id), shadow, "TOP_MAIN", { kind: "tool", name: "common.Control.inch" });
  const ga = findNode(a.tree, group.id)!.node as typeof group;
  assert.equal(ga.children[ga.children.length - 1]!.kind === "tool" && (ga.children[ga.children.length - 1] as { name: string }).name, "common.Control.inch");
  const b = addEntry(select(tree, group.children[0]!.id), shadow, "TOP_MAIN", { kind: "tool", name: "common.Control.inch" });
  assert.deepEqual((findNode(b.tree, group.id)!.node as typeof group).children.map((c) => (c as { name: string }).name), ["common.Control.metricUnits", "common.Control.inch", "common.Control.mils"]);
  const c = addEntry(select(tree, null), shadow, "TOP_MAIN", { kind: "tool", name: "common.Control.inch" });
  assert.equal((c.tree.nodes[c.tree.nodes.length - 1] as { name: string }).name, "common.Control.inch");
  assert.equal(c.tree.selected, c.tree.nodes[c.tree.nodes.length - 1]!.id);
  // the zoom box: off TOP_AUX, and the grid box already in this tree moved to after the selection
  const d = addEntry(select(tree, tree.nodes[0]!.id), shadow, "TOP_MAIN", { kind: "control", name: "control.ZoomSelector" });
  assert.deepEqual(d.shadow.TOP_AUX, [{ type: "SEPARATOR" }]);
  const e = addEntry(select(tree, tree.nodes[0]!.id), shadow, "TOP_MAIN", { kind: "control", name: "control.GridSelector" });
  assert.deepEqual(parseToolbarTree(e.tree).filter((i) => i.type === "CONTROL"), [{ type: "CONTROL", name: "control.GridSelector" }]);
  assert.equal(parseToolbarTree(e.tree)[1]!.type, "CONTROL");
});

test("Delete, Move Up / Move Down, renaming a group", () => {
  const tree = populateToolbarTree(sample, ctx);
  const [first, group, grid] = tree.nodes as [ToolbarTree["nodes"][number], Extract<ToolbarTree["nodes"][number], { kind: "group" }>, ToolbarTree["nodes"][number]];
  // delete selects the row before; the first row deleted selects nothing; a group's first tool selects the group
  assert.equal(deleteSelected(select(tree, grid.id)).selected, group.id);
  assert.equal(deleteSelected(select(tree, first.id)).selected, null);
  const d = deleteSelected(select(tree, group.children[0]!.id));
  assert.equal(d.selected, group.id);
  assert.equal((findNode(d, group.id)!.node as typeof group).children.length, 1);
  assert.equal(deleteSelected(select(tree, group.id)).nodes.length, tree.nodes.length - 1);
  // moves stay among siblings
  const up = moveSelected(select(tree, grid.id), -1);
  assert.deepEqual(up.nodes.map((n) => n.id).slice(0, 3), [first.id, grid.id, group.id]);
  assert.deepEqual(moveSelected(select(tree, first.id), -1), select(tree, first.id));
  const inGroup = moveSelected(select(tree, group.children[0]!.id), 1);
  assert.deepEqual((findNode(inGroup, group.id)!.node as typeof group).children.map((c) => c.id), [group.children[1]!.id, group.children[0]!.id]);
  assert.deepEqual(moveSelected(select(tree, group.children[1]!.id), 1), select(tree, group.children[1]!.id));
  // rename: KiCad 25ec2335 keeps the name and refuses an empty one; a non-group cannot be renamed
  assert.equal((findNode(renameGroup(tree, group.id, "My units")!, group.id)!.node as typeof group).name, "My units");
  assert.equal(renameGroup(tree, group.id, "   "), null);
  assert.equal(renameGroup(tree, first.id, "x"), null);
});

test("TransferDataFromWindow: a control stays on the first toolbar it is on", () => {
  const out = dedupeControls({
    TOP_MAIN: [{ type: "CONTROL", name: "control.GridSelector" }],
    LEFT: [{ type: "CONTROL", name: "control.GridSelector" }, { type: "SEPARATOR" }],
    TOP_AUX: [{ type: "CONTROL", name: "control.ZoomSelector" }, { type: "CONTROL", name: "control.ZoomSelector" }],
  });
  assert.deepEqual(out, { LEFT: [{ type: "CONTROL", name: "control.GridSelector" }, { type: "SEPARATOR" }], TOP_MAIN: [], TOP_AUX: [{ type: "CONTROL", name: "control.ZoomSelector" }] });
});
