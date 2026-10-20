import { test } from "node:test";
import assert from "node:assert/strict";
import type { Group, Schematic } from "../api/types";
import { boxSelectionWithGroups, describeGroup, groupBoundsOf, groupMenuFlags, groupableOf, leafIds, ownerOf, planGroup, substituteSchSelection, type BoxOf } from "./schGroups";
import { openGroups } from "./schDelete";
import { newIdsAfter, previewIds, withPaste, type SchPastePreview } from "./schClipboard";
import { keepOnSheet } from "./schSelectionPrune";
import type { Box } from "./schItemGeom";

const g = (id: string, name: string, members: string[]): Group => ({ id, name, member_ids: members });

/** A sheet of R1, R2, R3 (symbols), a wire, a label, a text and a sheet with a pin; `groups` as given. */
const sheet = (groups: Group[]): Schematic =>
  ({
    symbols: [{ id: "R1", unit: 1 }, { id: "R2", unit: 1 }, { id: "R3", unit: 1 }],
    wires: [{ id: "w1" }],
    labels: [{ id: "l1" }],
    texts: [{ id: "t1" }],
    power_symbols: [{ id: "#PWR01" }],
    no_connects: [],
    bus_entries: [],
    junctions: [],
    lines: [],
    graphics: [],
    sheets: [{ id: "sheet_a", pins: [{ id: "shpin_a" }], at: [0, 0], size: [10, 10] }],
    groups,
  }) as unknown as Schematic;

/** inner = {R1, R2}; outer = {inner, w1}. */
const nested = () => sheet([g("grp_inner", "pair", ["R1", "R2"]), g("grp_outer", "all", ["grp_inner", "w1"])]);

// boxes: R1 (0..10), R2 (20..30), R3 (100..110), w1 (10..20 in x, a thin one), l1 far away
const boxes: Record<string, Box> = {
  R1: { minX: 0, minY: 0, maxX: 10, maxY: 10 },
  R2: { minX: 20, minY: 0, maxX: 30, maxY: 10 },
  R3: { minX: 100, minY: 0, maxX: 110, maxY: 10 },
  w1: { minX: 10, minY: 5, maxX: 20, maxY: 5 },
  l1: { minX: 200, minY: 0, maxX: 210, maxY: 10 },
};
const boxOf: BoxOf = (id) => boxes[id] ?? null;

test("a pin, a field and a sheet pin follow the item they belong to", () => {
  const sch = nested();
  assert.equal(ownerOf(sch, "pin:R1:1"), "R1");
  assert.equal(ownerOf(sch, "pin:R2#2:3"), "R2");
  assert.equal(ownerOf(sch, "fld:R1:Reference"), "R1");
  assert.equal(ownerOf(sch, "fld:R2#2:Value"), "R2");
  assert.equal(ownerOf(sch, "shpin_a"), "sheet_a");
  assert.equal(ownerOf(sch, "w1"), "w1");
});

test("a click on a member selects the outermost group, a group entered lets its own members through", () => {
  const sch = nested();
  assert.deepEqual(substituteSchSelection(sch, ["R1"], null), { ids: ["grp_outer"], exits: false }, "R1 is in inner, which is in outer");
  assert.deepEqual(substituteSchSelection(sch, ["w1"], null), { ids: ["grp_outer"], exits: false });
  assert.deepEqual(substituteSchSelection(sch, ["R3"], null), { ids: ["R3"], exits: false }, "an item in no group is itself");
  // a pin of a grouped symbol is its symbol's group
  assert.deepEqual(substituteSchSelection(sch, ["pin:R1:1"], null).ids, ["grp_outer"]);
  assert.deepEqual(substituteSchSelection(sch, ["fld:R2:Value"], null).ids, ["grp_outer"]);
  // with the outer group entered, its direct members are the items: the inner group stands for R1 and R2, w1 is w1
  assert.deepEqual(substituteSchSelection(sch, ["R1", "w1"], "grp_outer"), { ids: ["grp_inner", "w1"], exits: false });
  // with the inner group entered, R1 is R1
  assert.deepEqual(substituteSchSelection(sch, ["R1"], "grp_inner"), { ids: ["R1"], exits: false });
  // an item outside the entered group leaves it, and is then picked as with none entered
  assert.deepEqual(substituteSchSelection(sch, ["R3"], "grp_inner"), { ids: ["R3"], exits: true });
  assert.deepEqual(substituteSchSelection(sch, ["w1"], "grp_inner"), { ids: ["grp_outer"], exits: true });
  // a sheet with no groups is untouched, and so is the selection a missing sheet gives
  assert.deepEqual(substituteSchSelection(sheet([]), ["R1", "R2"], null), { ids: ["R1", "R2"], exits: false });
  assert.deepEqual(substituteSchSelection(null, ["R1"], null), { ids: ["R1"], exits: false });
});

test("a window takes a group whole or not at all; a crossing box takes any member's group", () => {
  const sch = nested();
  const hits = (ids: string[]) => ids;
  // window around R1, R2 and w1 (the whole of outer): outer
  const around = { minX: -5, minY: -5, maxX: 35, maxY: 15 };
  assert.deepEqual(boxSelectionWithGroups(sch, hits(["R1", "R2", "w1"]), around, false, null, boxOf), ["grp_outer"]);
  // window around R1 and R2 only: inner's box (0..30) is inside, outer's is too (w1 is between them): the same
  // window around just R1: its group's box is not inside, so R1 on its own is not selected either
  assert.deepEqual(boxSelectionWithGroups(sch, hits(["R1"]), { minX: -5, minY: -5, maxX: 15, maxY: 15 }, false, null, boxOf), []);
  // a window around R3, in no group: R3
  assert.deepEqual(boxSelectionWithGroups(sch, hits(["R3"]), { minX: 90, minY: -5, maxX: 120, maxY: 15 }, false, null, boxOf), ["R3"]);
  // a crossing box that touches R1 only: the outermost group of R1
  assert.deepEqual(boxSelectionWithGroups(sch, hits(["R1"]), { minX: -5, minY: -5, maxX: 15, maxY: 15 }, true, null, boxOf), ["grp_outer"]);
  // R3 and a member: the group and R3
  assert.deepEqual(boxSelectionWithGroups(sch, hits(["R1", "R3"]), { minX: -5, minY: -5, maxX: 120, maxY: 15 }, true, null, boxOf), ["grp_outer", "R3"]);
  // the entered group's own members are picked one by one, also by a window; what is outside it is left out
  assert.deepEqual(boxSelectionWithGroups(sch, hits(["R1", "R2", "R3"]), { minX: -5, minY: -5, maxX: 120, maxY: 15 }, false, "grp_inner", boxOf), ["R1", "R2"]);
  // a pin the box found is taken as a pin, unless its group is selected too
  assert.deepEqual(boxSelectionWithGroups(sch, ["pin:R1:1"], { minX: -5, minY: -5, maxX: 15, maxY: 15 }, true, null, boxOf), ["pin:R1:1"]);
  assert.deepEqual(boxSelectionWithGroups(sch, ["R1", "pin:R1:1"], { minX: -5, minY: -5, maxX: 15, maxY: 15 }, true, null, boxOf), ["grp_outer"]);
});

test("a window that holds a group inside a bigger one selects the bigger one, as KiCad does", () => {
  // inner = {R1, R2} sits in outer = {inner, R3}: outer's box reaches R3, far off; a window around R1 and R2 holds inner's box and not outer's
  const sch = sheet([g("grp_inner", "pair", ["R1", "R2"]), g("grp_outer", "all", ["grp_inner", "R3"])]);
  const window = { minX: -5, minY: -5, maxX: 35, maxY: 15 };
  // inner's members come in, and `TopLevelGroup` promotes them to the outermost group: "if( EDA_GROUP* top = SCH_GROUP::TopLevelGroup( start, m_enteredGroup, .. ) )"
  assert.deepEqual(boxSelectionWithGroups(sch, ["R1", "R2"], window, false, null, boxOf), ["grp_outer"]);
});

test("Group takes at least two groupable items and leaves out what belongs to a symbol or a sheet", () => {
  const sch = nested();
  assert.equal(planGroup(sch, ["R3"]), null);
  assert.equal(planGroup(sch, ["R3", "pin:R3:1"]), null, "a pin is no item to group");
  assert.deepEqual(planGroup(sch, ["R3", "pin:R3:1", "fld:R3:Value", "shpin_a", "l1", "R3"]), ["R3", "l1"]);
  assert.deepEqual(planGroup(sch, ["grp_outer", "R3"]), ["grp_outer", "R3"], "a group is a member like any other");
  assert.equal(planGroup(sch, ["nope", "R3"]), null, "an id the sheet does not have is nothing");
  assert.deepEqual(groupableOf(sch, ["#PWR01", "t1", "sheet_a"]), ["#PWR01", "t1", "sheet_a"]);
});

test("the Grouping menu is on as GROUP_CONTEXT_MENU::update says", () => {
  const sch = nested();
  assert.deepEqual(groupMenuFlags(sch, ["R3", "l1"]), { count: 2, hasGroup: false, onlyOneGroup: false, hasUngroupedItems: true, hasMember: false });
  assert.deepEqual(groupMenuFlags(sch, ["grp_outer"]), { count: 1, hasGroup: true, onlyOneGroup: true, hasUngroupedItems: false, hasMember: false });
  assert.equal(groupMenuFlags(sch, ["grp_inner"]).hasMember, true, "the inner group is a member of the outer one");
  // one group and an ungrouped item: Add Items is on
  const one = groupMenuFlags(sch, ["grp_outer", "R3"]);
  assert.equal(one.onlyOneGroup && one.hasUngroupedItems, true);
  // two groups: Add Items is off ("Only allow one group to be selected for adding to existing group")
  const two = groupMenuFlags(sheet([g("a", "", ["R1", "R2"]), g("b", "", ["R3", "w1"])]), ["a", "b", "l1"]);
  assert.equal(two.onlyOneGroup, false);
  assert.equal(two.hasGroup, true);
  // a member of a group (by its symbol, for a pin) can be removed
  assert.equal(groupMenuFlags(sch, ["pin:R1:1"]).hasMember, true);
  assert.equal(groupMenuFlags(sch, ["R3"]).hasMember, false);
});

test("a group stands for the items below it, nested groups opened", () => {
  const sch = nested();
  assert.deepEqual(leafIds(sch, ["grp_outer"]), ["R1", "R2", "w1"]);
  assert.deepEqual(leafIds(sch, ["grp_inner", "R3", "R1"]), ["R1", "R2", "R3"]);
  assert.deepEqual(leafIds(sheet([]), ["grp_outer", "R3"]), ["grp_outer", "R3"], "with no groups nothing is opened");
});

test("a group's box is its members' box grown by 10 mil, and says nothing when nothing is left", () => {
  const groups = nested().groups!;
  assert.deepEqual(groupBoundsOf(groups, "grp_inner", boxOf), { minX: -254, minY: -254, maxX: 30 + 254, maxY: 10 + 254 });
  assert.deepEqual(groupBoundsOf(groups, "grp_outer", boxOf), { minX: -254, minY: -254, maxX: 30 + 254, maxY: 10 + 254 });
  assert.equal(groupBoundsOf(groups, "grp_inner", () => null), null);
  assert.equal(groupBoundsOf(groups, "R1", boxOf), null, "an item is no group");
});

test("a group is described as GetItemDescription says", () => {
  assert.equal(describeGroup(g("a", "power", ["x", "y", "z"])), "Group 'power', 3 members");
  assert.equal(describeGroup(g("a", "", ["x", "y"])), "Unnamed Group, 2 members");
});

test("Delete opens a group to its items, and a group with a locked item below it goes nowhere", () => {
  const groups = nested().groups!;
  assert.deepEqual(openGroups(groups, ["grp_outer", "R3"], new Set()), ["R1", "R2", "w1", "R3"]);
  assert.deepEqual(openGroups(groups, ["grp_outer", "R3"], new Set(["R2"])), ["R3"], "R2 is locked: the whole of outer is no selection");
  assert.deepEqual(openGroups(groups, ["grp_outer", "R3"], new Set(["grp_inner"])), ["R3"], "a locked group below it");
  assert.deepEqual(openGroups(groups, ["R1"], new Set(["grp_outer"])), ["R1"], "an item is as it is (the lock filter is the caller's)");
  assert.deepEqual(openGroups([], ["R1", "R2"], new Set()), ["R1", "R2"]);
});

test("the selection keeps a group that is still on the sheet", () => {
  const sch = nested();
  assert.deepEqual([...keepOnSheet(new Set(["grp_outer", "grp_gone"]), sch)], ["grp_outer"]);
});

test("a paste brings its groups: drawn selected while carried and selected as groups afterwards", () => {
  const before = nested();
  const preview: SchPastePreview = {
    symbols: [{ id: "R4", unit: 1, at: [0, 0] }, { id: "R5", unit: 1, at: [10, 0] }] as unknown as SchPastePreview["symbols"],
    power_symbols: [],
    wires: [],
    labels: [],
    texts: [],
    no_connects: [],
    bus_entries: [],
    junctions: [],
    lines: [],
    graphics: [],
    groups: [g("grp_new", "copy", ["R4", "R5"])],
    lib_symbols: {},
  };
  assert.deepEqual(previewIds(preview).sort(), ["R4", "R5", "grp_new"]);
  const shown = withPaste(before, preview, 0, 0);
  assert.deepEqual(shown.groups?.map((x) => x.id), ["grp_inner", "grp_outer", "grp_new"]);
  // once placed: the new items are selected as the group they are in
  const after = { ...before, symbols: [...before.symbols, ...preview.symbols], groups: [...(before.groups ?? []), ...(preview.groups ?? [])] } as Schematic;
  assert.deepEqual(newIdsAfter(before, after), ["grp_new"]);
});
