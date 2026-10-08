import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultDockLayout, NARROW_WINDOW_PX, paneVisible, parseDockLayout, schLeftColumn, selectionFilterShown, setColumnCollapsed, toggleFolded, togglePane } from "./dockLayout";

test("a wide window starts with every pane open; an 800 px window starts with both columns folded so the canvas keeps the width", () => {
  const wide = defaultDockLayout(1600);
  assert.equal(wide.leftCollapsed, false);
  assert.equal(wide.rightCollapsed, false);
  const narrow = defaultDockLayout(800);
  assert.equal(narrow.leftCollapsed, true);
  assert.equal(narrow.rightCollapsed, true);
  assert.equal(defaultDockLayout(NARROW_WINDOW_PX).leftCollapsed, false, "the threshold itself is wide");
  assert.equal(defaultDockLayout(NARROW_WINDOW_PX - 1).leftCollapsed, true);
});

test("the schematic's left column holds Hierarchy, Properties and Selection Filter in KiCad's Position order, with the Net Navigator above them when it is open", () => {
  assert.deepEqual(schLeftColumn(defaultDockLayout(1600)), ["hierarchy", "properties", "selectionFilter"]);
  assert.deepEqual(schLeftColumn(defaultDockLayout(1600), true), ["netNavigator", "hierarchy", "properties", "selectionFilter"], "Position 0: the first pane of the column");
});

test("the Net Navigator alone keeps the Selection Filter on screen (updateSelectionFilterVisbility counts it)", () => {
  let l = defaultDockLayout(1600);
  l = togglePane(togglePane(l, "hierarchy"), "properties");
  assert.equal(selectionFilterShown(l), false);
  assert.equal(selectionFilterShown(l, true), true);
  assert.deepEqual(schLeftColumn(l, true), ["netNavigator", "selectionFilter"]);
});

test("the Selection Filter has no switch of its own: it shows while the hierarchy or the properties pane is shown", () => {
  let l = defaultDockLayout(1600);
  assert.equal(selectionFilterShown(l), true);
  l = togglePane(l, "hierarchy");
  assert.equal(paneVisible(l, "hierarchy"), false);
  assert.equal(selectionFilterShown(l), true, "properties still shown");
  l = togglePane(l, "properties");
  assert.equal(selectionFilterShown(l), false, "nothing else is shown, so the filter goes too");
  assert.deepEqual(schLeftColumn(l), []);
  l = togglePane(l, "hierarchy");
  assert.deepEqual(schLeftColumn(l), ["hierarchy", "selectionFilter"]);
});

test("showing a pane unfolds it and brings its column back; hiding one leaves the column alone", () => {
  let l = defaultDockLayout(800); // both columns folded
  l = togglePane(l, "properties"); // hide
  assert.equal(l.leftCollapsed, true, "hiding does not open the column");
  l = togglePane(l, "properties"); // show again
  assert.equal(l.shown.properties, true);
  assert.equal(l.leftCollapsed, false, "showing it opens the column, or the button would look dead");
  assert.equal(l.rightCollapsed, true, "the other column is untouched");
});

test("folding a pane keeps it shown; folding is independent per pane", () => {
  let l = defaultDockLayout(1600);
  l = toggleFolded(l, "properties");
  assert.equal(l.folded.properties, true);
  assert.equal(l.shown.properties, true);
  assert.equal(l.folded.hierarchy, false);
  l = toggleFolded(l, "properties");
  assert.equal(l.folded.properties, false);
});

test("collapsing one column leaves the other and the panes alone", () => {
  const l = setColumnCollapsed(defaultDockLayout(1600), "left", true);
  assert.equal(l.leftCollapsed, true);
  assert.equal(l.rightCollapsed, false);
  assert.equal(l.shown.properties, true);
});

test("the library editors' tree column folds on its own and starts open even in a narrow window (the tree is how an item is opened)", () => {
  assert.equal(defaultDockLayout(800).treeCollapsed, false);
  const l = setColumnCollapsed(defaultDockLayout(800), "tree", true);
  assert.equal(l.treeCollapsed, true);
  assert.equal(l.leftCollapsed, true, "the frame's own left column keeps its own state");
  assert.equal(setColumnCollapsed(l, "tree", false).treeCollapsed, false);
  assert.equal(parseDockLayout({ treeCollapsed: true }, 1600).treeCollapsed, true);
});

test("a stored layout is read back leniently: wrong types and missing fields fall back to the default for the window", () => {
  assert.deepEqual(parseDockLayout(null, 800), defaultDockLayout(800));
  assert.deepEqual(parseDockLayout("nope", 1600), defaultDockLayout(1600));
  const stored = parseDockLayout({ leftCollapsed: false, shown: { properties: false, hierarchy: "yes" }, folded: { selectionFilter: true } }, 800);
  assert.equal(stored.leftCollapsed, false, "the stored value wins over the width-based default");
  assert.equal(stored.rightCollapsed, true, "a missing field takes the default for this window");
  assert.equal(stored.shown.properties, false);
  assert.equal(stored.shown.hierarchy, true, "a non-boolean is ignored");
  assert.equal(stored.folded.selectionFilter, true);
});
