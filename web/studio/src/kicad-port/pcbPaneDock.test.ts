import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CAPTION_PX,
  PANE_FLOATING_SIZE,
  clampDockSize,
  clampPaneRect,
  defaultPcbPaneDock,
  dockGuideAt,
  dockGuides,
  dockHintRect,
  floatingPanes,
  movePane,
  paneDockChoices,
  panesOnSide,
  parsePcbPaneDock,
  toggledPaneDock,
} from "./pcbPaneDock";

const all = () => true;

test("both panes start .Bottom(), in the order PCB_EDIT_FRAME adds them; a kept bottom height is where the dock starts", () => {
  const d = defaultPcbPaneDock();
  assert.deepEqual(d.at, { netInspector: "bottom", search: "bottom" });
  assert.deepEqual(panesOnSide(d, "bottom", all), ["netInspector", "search"]);
  assert.deepEqual(panesOnSide(d, "bottom", (id) => id === "search"), ["search"], "a hidden pane takes no place in its dock");
  assert.equal(defaultPcbPaneDock(300).size.bottom, 300);
  assert.deepEqual({ w: d.floating.search.w, h: d.floating.search.h }, PANE_FLOATING_SIZE.search, "FloatingSize( 480, 200 )");
  assert.deepEqual({ w: d.floating.netInspector.w, h: d.floating.netInspector.h }, { w: 300, h: 200 });
});

test("a pane docks on any side of the canvas or floats; a double click floats a docked pane and docks a floating one back where it came from", () => {
  let d = movePane(defaultPcbPaneDock(), "search", "left");
  assert.deepEqual(panesOnSide(d, "left", all), ["search"]);
  assert.deepEqual(panesOnSide(d, "bottom", all), ["netInspector"]);
  assert.equal(d.lastDocked.search, "left");
  assert.equal(toggledPaneDock(d, "search"), "float");
  d = movePane(d, "search", "float", { x: 40.4, y: 70.6 });
  assert.deepEqual(floatingPanes(d, all), ["search"]);
  assert.deepEqual([d.floating.search.x, d.floating.search.y], [40, 71], "the window opens where the pane was");
  assert.equal(toggledPaneDock(d, "search"), "left", "wxAuiPaneInfo::Dock() keeps the direction it floated from");
  d = movePane(d, "netInspector", "left");
  assert.deepEqual(panesOnSide(d, "left", all), ["netInspector"], "the floating one is in no dock");
  d = movePane(d, "search", "left");
  assert.deepEqual(panesOnSide(d, "left", all), ["netInspector", "search"], "two panes in one dock: in the order they were added");
});

test("a kept layout reads back; anything of the wrong kind keeps its default", () => {
  const kept = movePane(movePane(defaultPcbPaneDock(), "netInspector", "right"), "search", "float", { x: 10, y: 20 });
  kept.size.right = 420;
  assert.deepEqual(parsePcbPaneDock(JSON.parse(JSON.stringify(kept))), kept);
  const bad = parsePcbPaneDock({ at: { netInspector: "middle", search: 3 }, size: { left: -5, top: "x" }, floating: { search: { x: 1 } } });
  assert.deepEqual(bad, defaultPcbPaneDock());
  assert.deepEqual(parsePcbPaneDock(null, 250).size.bottom, 250);
});

test("a dock's sash: never under its panes' MinSize (with the caption for a top or bottom dock), never so deep the canvas goes", () => {
  assert.equal(clampDockSize("bottom", 10, ["netInspector", "search"], 1600, 1000), 60 + CAPTION_PX);
  assert.equal(clampDockSize("left", 10, ["netInspector"], 1600, 1000), 240, "the Net Inspector's MinSize( 240, 60 )");
  assert.equal(clampDockSize("left", 10, ["search"], 1600, 1000), 180, "Search's MinSize( 180, 60 )");
  assert.equal(clampDockSize("bottom", 5000, ["search"], 1600, 1000), 800);
  assert.equal(clampDockSize("right", 5000, ["search"], 1600, 1000), 1240);
  assert.equal(clampDockSize("top", 300.4, ["search"], 1600, 1000), 300);
});

test("a floating window stays on the page, no smaller than its pane's MinSize", () => {
  assert.deepEqual(clampPaneRect("search", { x: -50, y: 950, w: 100, h: 10 }, 1600, 1000), { x: 0, y: 1000 - 82, w: 180, h: 82 });
  assert.deepEqual(clampPaneRect("netInspector", { x: 1500, y: 10, w: 300, h: 200 }, 1600, 1000), { x: 1300, y: 10, w: 300, h: 200 });
});

test("the centre dock guide: an arrow each side of the canvas's centre; a drop on one docks there, the hint is that part of the canvas", () => {
  const canvas = { left: 100, top: 50, width: 800, height: 600 };
  const g = Object.fromEntries(dockGuides(canvas).map((x) => [x.side, x.rect]));
  const centre = (r: { left: number; top: number; width: number; height: number }) => [r.left + r.width / 2, r.top + r.height / 2] as const;
  for (const side of ["top", "bottom", "left", "right"] as const) assert.equal(dockGuideAt(canvas, ...centre(g[side]!)), side);
  assert.equal(dockGuideAt(canvas, 500, 350), null, "the centre itself is no side (the centre guide docks a notebook tab, which these panes are not)");
  assert.equal(dockGuideAt(canvas, 120, 70), null);
  assert.ok(centre(g.top!)[1] < 350 && centre(g.bottom!)[1] > 350 && centre(g.left!)[0] < 500 && centre(g.right!)[0] > 500);
  assert.deepEqual(dockHintRect(canvas, "bottom", 220), { left: 100, top: 430, width: 800, height: 220 });
  assert.deepEqual(dockHintRect(canvas, "left", 300), { left: 100, top: 50, width: 300, height: 600 });
  assert.deepEqual(dockHintRect(canvas, "right", 2000), { left: 500, top: 50, width: 400, height: 600 }, "half the canvas at most");
});

test("the caption's buttons: the other places the pane may go, then its close button", () => {
  assert.deepEqual(
    paneDockChoices("bottom", "Search").map((c) => c.to),
    ["top", "left", "right", "float", "close"]
  );
  assert.deepEqual(
    paneDockChoices("float", "Net Inspector").map((c) => c.label),
    ["Dock at the bottom", "Dock at the top", "Dock on the left", "Dock on the right", "Hide Net Inspector"]
  );
});
