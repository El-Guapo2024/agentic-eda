import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CAPTION_PX,
  DEFAULT_PROPORTION,
  PANE_FLOATING_SIZE,
  clampDockSize,
  clampPaneRect,
  defaultPaneDock,
  doDrop,
  dockBack,
  dockOnSide,
  dockRows,
  dropHintRect,
  floatGrab,
  floatPane,
  floatingPanes,
  newDockDepth,
  outerLayers,
  paneDockChoices,
  parsePaneDock,
  resizeBetween,
  setRowDepth,
  sideRows,
  toggledPaneDock,
  type DropGeometry,
  type MovablePaneId,
  type PaneDock,
} from "./auiDock";

const all = () => true;
const rect = (left: number, top: number, width: number, height: number) => ({ left, top, width, height });

/** A 1600 x 1000 editor: the frame from y 60 to 980, the canvas 200..1300 x 100..700, the panes under it, the toolbars round it. */
function geometry(dock: PaneDock, panes: Partial<Record<MovablePaneId, ReturnType<typeof rect>>> = {}): DropGeometry {
  void dock;
  return {
    frame: rect(0, 60, 1600, 920),
    canvas: rect(200, 100, 1100, 600),
    panes: Object.entries(panes).map(([id, r]) => ({ id: id as MovablePaneId, rect: r! })),
    sashes: [],
    toolbars: [
      { side: "top", rect: rect(0, 60, 1600, 30) },
      { side: "left", rect: rect(160, 100, 40, 820) },
      { side: "right", rect: rect(1300, 100, 40, 820) },
    ],
  };
}

test("every pane starts .Bottom() at layer 0, row 0, in the frame's order; the dock as deep as KiCad's new dock", () => {
  const d = defaultPaneDock("pcb");
  assert.deepEqual(d.at, { netInspector: "bottom", search: "bottom" });
  assert.deepEqual(dockRows("pcb", d, all).map((r) => [r.side, r.layer, r.row, r.panes]), [["bottom", 0, 0, ["netInspector", "search"]]]);
  assert.deepEqual(dockRows("pcb", d, (id) => id === "search")[0]!.panes, ["search"], "a hidden pane takes no place in its dock");
  assert.equal(dockRows("pcb", defaultPaneDock("pcb", 300), all)[0]!.depth, 300, "a kept bottom height");
  assert.deepEqual({ w: d.floating.search!.w, h: d.floating.search!.h }, PANE_FLOATING_SIZE.search, "FloatingSize( 480, 200 )");
  const s = defaultPaneDock("sch");
  assert.deepEqual(s.at, { schSearch: "bottom" });
  assert.equal(s.depth.schSearch, 100 + CAPTION_PX, "BestSize( 180, 100 ) plus the caption");
  assert.equal(d.proportion.netInspector, DEFAULT_PROPORTION);
});

test("a layout kept before the rows is read back: each pane on row 0 of its side, the side's size its depth", () => {
  const d = parsePaneDock("pcb", { at: { netInspector: "left", search: "float" }, lastDocked: { search: "right" }, size: { left: 333, right: 250, bottom: 220 }, floating: { search: { x: 1, y: 2, w: 500, h: 210 } } });
  assert.equal(d.at.netInspector, "left");
  assert.deepEqual(d.spot.netInspector, { layer: 0, row: 0, pos: 0 });
  assert.equal(d.depth.netInspector, 333);
  assert.equal(d.depth.search, 250, "a floating pane's depth is its last side's");
  assert.deepEqual(d.floating.search, { x: 1, y: 2, w: 500, h: 210 });
  assert.deepEqual(parsePaneDock("pcb", "junk"), defaultPaneDock("pcb"));
  assert.deepEqual(parsePaneDock("pcb", { at: { netInspector: "nowhere" }, spot: { netInspector: { layer: -1, row: 0, pos: 0 } } }).at.netInspector, "bottom");
});

test("rows go inwards from the frame's edge: a bottom or right side's are laid out from the canvas out", () => {
  let d = defaultPaneDock("pcb");
  d = { ...d, spot: { netInspector: { layer: 0, row: 0, pos: 0 }, search: { layer: 0, row: 1, pos: 0 } } };
  const rows = dockRows("pcb", d, all);
  assert.deepEqual(sideRows(rows, "bottom", 0).map((r) => r.row), [1, 0], "row 0 at the bottom edge, row 1 by the canvas");
  d = { ...d, at: { netInspector: "top", search: "top" } };
  assert.deepEqual(sideRows(dockRows("pcb", d, all), "top", 0).map((r) => r.row), [0, 1]);
  assert.deepEqual(outerLayers(dockRows("pcb", { ...d, spot: { netInspector: { layer: 2, row: 0, pos: 0 }, search: { layer: 1, row: 0, pos: 0 } } }, all)), [1, 2]);
});

test("DoDrop: along an edge of the frame the pane docks on a layer of its own, one above the highest of that side and the two beside it", () => {
  const d = floatPane(defaultPaneDock("pcb"), "search");
  const g = geometry(d);
  const left = doDrop("pcb", d, "search", g, 2, 500, all);
  assert.equal(left?.how, "edge");
  assert.deepEqual([left!.dock.at.search, left!.dock.spot.search], ["left", { layer: 1, row: 0, pos: 0 }]);
  assert.equal(left!.dock.depth.search, 180, "a new dock: BestSize's width");
  // the Net Inspector at layer 1 on the left: a drop at the top edge goes round it, on layer 2
  const ni = { ...left!.dock, at: { ...left!.dock.at, netInspector: "left" as const, search: "float" as const }, spot: { ...left!.dock.spot, netInspector: { layer: 1, row: 0, pos: 0 } } };
  const top = doDrop("pcb", ni, "search", g, 800, 62, all);
  assert.deepEqual([top?.how, top!.dock.at.search, top!.dock.spot.search!.layer], ["edge", "top", 2]);
  assert.equal(doDrop("pcb", d, "search", g, 800, 60 - 34, all)?.side, "top", "up to 35 px outside the frame");
  assert.equal(doDrop("pcb", d, "search", g, 800, 60 - 36, all), null);
  assert.equal(doDrop("pcb", d, "search", g, 1598, 500, all)?.side, "right");
  assert.equal(doDrop("pcb", d, "search", g, 800, 978, all)?.side, "bottom");
});

test("DoDrop over a docked pane: by its outer edge a new row there, the others moved in; elsewhere before or after it in its row", () => {
  const d = floatPane(defaultPaneDock("pcb"), "search");
  // the Net Inspector alone under the canvas: x 200..1300, y 700..920
  const g = geometry(d, { netInspector: rect(200, 700, 1100, 220) });
  const before = doDrop("pcb", d, "search", g, 300, 800, all);
  assert.deepEqual([before?.how, before?.after, before!.dock.spot.search, before!.dock.spot.netInspector], ["pane", false, { layer: 0, row: 0, pos: 0 }, { layer: 0, row: 0, pos: 1 }]);
  assert.deepEqual(dockRows("pcb", before!.dock, all)[0]!.panes, ["search", "netInspector"]);
  const after = doDrop("pcb", d, "search", g, 1200, 800, all);
  assert.deepEqual(dockRows("pcb", after!.dock, all)[0]!.panes, ["netInspector", "search"]);
  assert.equal(after!.dock.depth.search, 220, "joining a dock: its depth");
  // the bottom 10 px of a bottom pane: a row of its own further out, the Net Inspector's row moved in
  const row = doDrop("pcb", d, "search", g, 700, 915, all);
  assert.deepEqual([row?.how, row!.dock.spot.search!.row, row!.dock.spot.netInspector!.row], ["row", 0, 1]);
  assert.deepEqual(sideRows(dockRows("pcb", row!.dock, all), "bottom", 0).map((r) => r.panes), [["netInspector"], ["search"]], "the Net Inspector by the canvas, Search at the edge");
  // a left dock: one over the other, by the top and bottom halves
  const left = { ...d, at: { ...d.at, netInspector: "left" as const } };
  const gl = geometry(left, { netInspector: rect(200, 100, 300, 600) });
  assert.deepEqual(dockRows("pcb", doDrop("pcb", left, "search", gl, 350, 650, all)!.dock, all)[0]!.panes, ["netInspector", "search"]);
  assert.equal(doDrop("pcb", left, "search", gl, 203, 400, all)?.how, "row", "a left pane's left 10 px");
});

test("DoDrop over a toolbar: along the edge, over the other panes; over the canvas, a new row by its edge; elsewhere it floats", () => {
  const d = floatPane(defaultPaneDock("pcb"), "search");
  const g = geometry(d);
  const bar = doDrop("pcb", d, "search", g, 180, 400, all);
  assert.deepEqual([bar?.how, bar!.dock.at.search, bar!.dock.spot.search], ["toolbar", "left", { layer: 1, row: 0, pos: 0 }]);
  const canvas = doDrop("pcb", d, "search", g, 1290, 400, all);
  assert.deepEqual([canvas?.how, canvas!.dock.at.search, canvas!.dock.spot.search!.layer], ["canvas", "right", 0]);
  // under the canvas, inside the Net Inspector's row (GetMaxRow + 1)
  const under = doDrop("pcb", d, "search", g, 700, 690, all);
  assert.deepEqual([under!.dock.at.search, under!.dock.spot.search!.row], ["bottom", 1]);
  assert.deepEqual(sideRows(dockRows("pcb", under!.dock, all), "bottom", 0).map((r) => r.panes), [["search"], ["netInspector"]], "Search by the canvas");
  assert.equal(doDrop("pcb", d, "search", g, 750, 400, all), null, "the middle of the canvas");
  // the sash of a dock of one pane is that pane's
  const gs: DropGeometry = { ...geometry(d, { netInspector: rect(200, 704, 1100, 216) }), sashes: [{ rect: rect(200, 700, 1100, 4), panes: ["netInspector"] }] };
  assert.equal(doDrop("pcb", d, "search", gs, 700, 701, all)?.by, "netInspector");
});

test("a double click floats a docked pane and docks it back where it was: its side, layer, row and position", () => {
  let d = defaultPaneDock("pcb");
  d = { ...d, spot: { ...d.spot, search: { layer: 0, row: 1, pos: 0 } } };
  assert.equal(toggledPaneDock(d, "search"), "float");
  const f = floatPane(d, "search", { x: 40.4, y: 70.6 });
  assert.deepEqual(floatingPanes("pcb", f, all), ["search"]);
  assert.deepEqual([f.floating.search!.x, f.floating.search!.y], [40, 71], "the window opens where the pane was");
  assert.equal(toggledPaneDock(f, "search"), "bottom");
  assert.deepEqual(dockBack("pcb", f, "search").spot.search, { layer: 0, row: 1, pos: 0 });
  assert.deepEqual(floatGrab({ x: 520, y: 8 }, 480), { x: 30, y: 8 }, "grabbed further in than the window is wide: 30 px in");
  assert.deepEqual(floatGrab({ x: 50, y: 8 }, 480), { x: 50, y: 8 });
});

test("the caption's buttons: into the innermost dock of layer 0 on that side, after its panes, else a dock of its own", () => {
  let d = dockOnSide("pcb", defaultPaneDock("pcb"), "netInspector", "left", all, { width: 1600, height: 1000 });
  assert.deepEqual([d.at.netInspector, d.spot.netInspector, d.depth.netInspector], ["left", { layer: 0, row: 0, pos: 0 }, 300]);
  d = dockOnSide("pcb", d, "search", "left", all);
  assert.deepEqual(dockRows("pcb", d, all).map((r) => r.panes), [["netInspector", "search"]]);
  assert.equal(d.depth.search, 300, "it takes the dock's depth");
  assert.deepEqual(paneDockChoices("left", "Search").map((c) => c.to), ["bottom", "top", "right", "float", "close"]);
});

test("sashes: a dock's depth for its panes; the sizer between two shares their proportion; floating windows kept on the page", () => {
  const d = setRowDepth(defaultPaneDock("pcb"), ["netInspector", "search"], 300);
  assert.deepEqual([d.depth.netInspector, d.depth.search], [300, 300]);
  const r = resizeBetween(d, "bottom", "netInspector", "search", 500, 500, 100);
  assert.equal(r.proportion.netInspector! + r.proportion.search!, 2 * DEFAULT_PROPORTION);
  assert.equal(Math.round((r.proportion.netInspector! / (2 * DEFAULT_PROPORTION)) * 1000), 600);
  const min = resizeBetween(d, "bottom", "netInspector", "search", 500, 500, -480);
  assert.equal(Math.round((min.proportion.netInspector! / (2 * DEFAULT_PROPORTION)) * 1000), 240, "the Net Inspector's MinSize width, 240");
  assert.equal(clampDockSize("bottom", 10, ["netInspector"], 1600, 1000), 60 + CAPTION_PX);
  assert.equal(clampDockSize("left", 10, ["netInspector"], 1600, 1000), 240);
  assert.equal(clampDockSize("left", 5000, ["search"], 1600, 1000), 1240);
  assert.deepEqual(clampPaneRect("search", { x: -50, y: 900, w: 100, h: 50 }, 1600, 1000), { x: 0, y: 900, w: 180, h: 82 });
  assert.equal(newDockDepth("netInspector", "bottom", { width: 1600, height: 1000 }), 222);
  assert.equal(newDockDepth("netInspector", "bottom", { width: 1600, height: 300 }), 100, "a third of the frame at most");
});

test("the hint: a band along the frame's edge, the canvas's edge or the pane's outer edge; half of a pane for a place in its row", () => {
  const d = floatPane(defaultPaneDock("pcb"), "search");
  const g = geometry(d, { netInspector: rect(200, 700, 1100, 220) });
  const edge = doDrop("pcb", d, "search", g, 2, 500, all)!;
  assert.deepEqual(dropHintRect(edge, g, 180), rect(0, 60, 180, 920));
  const half = doDrop("pcb", d, "search", g, 1200, 800, all)!;
  assert.deepEqual(dropHintRect(half, g, 220), rect(750, 700, 550, 220));
  const canvas = doDrop("pcb", d, "search", g, 210, 400, all)!;
  assert.deepEqual(dropHintRect(canvas, g, 180), rect(200, 100, 180, 600));
});
