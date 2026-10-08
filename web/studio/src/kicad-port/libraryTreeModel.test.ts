import { test } from "node:test";
import assert from "node:assert/strict";
import { buildTreeGroups, defaultGroupOpen, needsLoading, SEARCH_RESULT_CAP, type InstalledLib, type TreeItem } from "./libraryTreeModel";

const known: TreeItem[] = [
  { name: "Untitled", project: true },
  { name: "Device:R", project: false },
  { name: "Device:LED", project: false },
];
const installed: InstalledLib[] = [{ name: "Package_SO", count: 3 }, { name: "Device" }, { name: "Connector" }];

test("every installed library is a group, collapsed with nothing loaded, and the project's own group comes first", () => {
  const { groups } = buildTreeGroups(known, installed, new Map(), "");
  assert.deepEqual(groups.map((g) => g.lib), ["eda", "Connector", "Device", "Package_SO"]);
  const so = groups.find((g) => g.lib === "Package_SO")!;
  assert.equal(so.items.length, 0);
  assert.equal(so.count, 3, "the count the server knew without opening the library");
  assert.equal(so.installed, true);
  assert.equal(defaultGroupOpen(so), false, "an installed library nobody asked for stays closed");
  assert.equal(defaultGroupOpen(groups[0]!), true, "the project's group is open");
  assert.equal(groups[0]!.installed, false, "`eda` is the project's own library, nothing installed to load");
});

test("a group the editor already lists names in starts open, and loads its installed items so the two lists are one", () => {
  const loaded = new Map([["Device", ["C", "LED", "R"]]]);
  const { groups } = buildTreeGroups(known, installed, loaded, "");
  const device = groups.find((g) => g.lib === "Device")!;
  assert.deepEqual(device.items.map((i) => i.name), ["Device:C", "Device:LED", "Device:R"], "known and installed items merge without duplicates, sorted");
  assert.equal(defaultGroupOpen(device), true, "it holds the model's Device:R and Device:LED");
  // before the library is loaded, an open group that KiCad has a library for is asked to load
  const before = buildTreeGroups(known, installed, new Map(), "").groups.find((g) => g.lib === "Device")!;
  assert.equal(needsLoading(before, true, new Map()), true);
  assert.equal(needsLoading(before, false, new Map()), false, "closed: nothing to load");
  assert.equal(needsLoading(device, true, loaded), false, "already loaded");
  assert.equal(needsLoading(buildTreeGroups(known, installed, new Map(), "").groups[0]!, true, new Map()), false, "the project library is not an installed library");
});

test("an item the project has stays a project item even when the installed library lists it too", () => {
  const { groups } = buildTreeGroups([{ name: "Package_SO:SOIC-8", project: true }], installed, new Map([["Package_SO", ["SOIC-8", "SOIC-16"]]]), "");
  assert.equal(groups[0]!.lib, "Package_SO", "a library holding a project entry is listed first");
  assert.equal(groups[0]!.hasProject, true);
  const items = groups[0]!.items;
  assert.deepEqual(items.map((i) => [i.name, i.project]), [["Package_SO:SOIC-8", true], ["Package_SO:SOIC-16", false]], "numbers sort as numbers, like the tree");
});

test("search matches the whole Lib:Name, case-insensitively, and shows a library that was never loaded by its own name", () => {
  const loaded = new Map([["Package_SO", ["SOIC-8", "SOIC-16", "TSSOP-20"]]]);
  const r = buildTreeGroups(known, installed, loaded, "soic");
  assert.deepEqual(r.groups.map((g) => g.lib), ["Package_SO"]);
  assert.deepEqual(r.groups[0]!.items.map((i) => i.name), ["Package_SO:SOIC-8", "Package_SO:SOIC-16"].sort((a, b) => a.localeCompare(b, undefined, { numeric: true })));
  // "conn": Connector was never loaded, so it cannot say whether it holds a match -- it shows by its name
  const byName = buildTreeGroups(known, installed, loaded, "conn");
  assert.deepEqual(byName.groups.map((g) => g.lib), ["Connector"]);
  assert.equal(byName.groups[0]!.items.length, 0);
  // once it IS loaded and holds nothing that matches, it goes
  const loadedAll = new Map([...loaded, ["Connector", ["Conn_01x04"]], ["Device", ["R"]]]);
  assert.deepEqual(buildTreeGroups(known, installed, loadedAll, "zzz").groups, []);
  assert.deepEqual(buildTreeGroups(known, installed, loadedAll, "01x04").groups.map((g) => g.lib), ["Connector"]);
});

test("a search is cut at the cap and says so; browsing is never cut", () => {
  const many = Array.from({ length: SEARCH_RESULT_CAP + 50 }, (_, i) => `Part_${String(i).padStart(4, "0")}`);
  const loaded = new Map([["Big", many]]);
  const searched = buildTreeGroups([], [{ name: "Big" }], loaded, "part");
  assert.equal(searched.truncated, true);
  assert.equal(searched.groups[0]!.items.length, SEARCH_RESULT_CAP);
  const browsed = buildTreeGroups([], [{ name: "Big" }], loaded, "");
  assert.equal(browsed.truncated, false);
  assert.equal(browsed.groups[0]!.items.length, many.length);
});

test("a bare name belongs to the project library, like the tree has always grouped it", () => {
  const { groups } = buildTreeGroups([{ name: "0603", project: false }], [], new Map(), "");
  assert.deepEqual(groups.map((g) => g.lib), ["eda"]);
});
