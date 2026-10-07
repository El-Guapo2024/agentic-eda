import { test } from "node:test";
import assert from "node:assert/strict";
import {
  afterFill,
  afterUnfill,
  connectedTrackWidth,
  displayedRatsnest,
  duplicatedZoneOutline,
  filledNow,
  filterZones,
  flipLocalX,
  flipPan,
  highlightedNets,
  isHighlighted,
  keepFilled,
  moveZone,
  netsOfSelection,
  panDeltaX,
  rankPriorities,
  ratsnestModeCycle,
  selectedCopperZones,
  setNetsHidden,
  swapHighlight,
  toggleLocalRatsnestFootprint,
  toggleLocalRatsnestPad,
  zoneOrder,
  type RatsnestView,
} from "./boardControl";
import type { FillReport, RatsnestEdge } from "../api/types";

test("ratsnestModeCycle: off -> all layers -> visible layers -> off, as PCB_CONTROL::RatsnestModeCycle", () => {
  let s = { show: false, mode: "all" as "all" | "visible" };
  s = ratsnestModeCycle(s.show, s.mode);
  assert.deepEqual(s, { show: true, mode: "all" });
  s = ratsnestModeCycle(s.show, s.mode);
  assert.deepEqual(s, { show: true, mode: "visible" });
  s = ratsnestModeCycle(s.show, s.mode);
  assert.deepEqual(s, { show: false, mode: "visible" }, "off keeps the mode it was in");
  // From off it always restarts at all layers, whatever the stored mode.
  assert.deepEqual(ratsnestModeCycle(false, "visible"), { show: true, mode: "all" });
});

const edge = (net: string, fromId: string, toId: string, fromLayers: [number, number] = [0, 0], toLayers: [number, number] = [0, 0]): RatsnestEdge => ({ net, from: [0, 0], to: [1, 1], from_id: fromId, to_id: toId, from_layers: fromLayers, to_layers: toLayers });

const view = (over: Partial<RatsnestView> = {}): RatsnestView => ({ showGlobal: true, mode: "all", hiddenNets: new Set(), flippedPads: new Set(), visibleLayers: new Set([0, 1]), ...over });

test("displayedRatsnest: everything shows with the global ratsnest on and nothing hidden", () => {
  const edges = [edge("A", "U1.1", "R1.1"), edge("B", "U1.2", "R1.2")];
  assert.equal(displayedRatsnest(edges, view()).length, 2);
});

test("displayedRatsnest: a hidden net draws nothing, global or not", () => {
  const edges = [edge("A", "U1.1", "R1.1"), edge("B", "U1.2", "R1.2")];
  assert.deepEqual(displayedRatsnest(edges, view({ hiddenNets: new Set(["A"]) })).map((e) => e.net), ["B"]);
  assert.deepEqual(displayedRatsnest(edges, view({ showGlobal: false, flippedPads: new Set(["U1.1", "U1.2"]), hiddenNets: new Set(["A"]) })).map((e) => e.net), ["B"]);
});

test("displayedRatsnest: with the global ratsnest on, a pad the Local Ratsnest tool clicked turns its lines OFF (either end can)", () => {
  const edges = [edge("A", "U1.1", "R1.1"), edge("B", "U1.2", "R1.2")];
  const shown = displayedRatsnest(edges, view({ flippedPads: new Set(["R1.1"]) }));
  assert.deepEqual(shown.map((e) => e.net), ["B"]);
});

test("displayedRatsnest: with it off, a pad the tool clicked turns its lines ON (either end can) and nothing else shows", () => {
  const edges = [edge("A", "U1.1", "R1.1"), edge("B", "U1.2", "R1.2"), edge("C", "trk_1", "via_1")];
  const shown = displayedRatsnest(edges, view({ showGlobal: false, flippedPads: new Set(["R1.1"]) }));
  assert.deepEqual(shown.map((e) => e.net), ["A"]);
  assert.deepEqual(displayedRatsnest(edges, view({ showGlobal: false })), [], "global off, nothing clicked: nothing");
});

test("displayedRatsnest: in visible-layers mode both ends must be on a layer that is shown; a through-hole pad is on all of them", () => {
  // Layers 0 (F.Cu) and 1 (B.Cu); only F.Cu is on show.
  const only = view({ mode: "visible", visibleLayers: new Set([0]) });
  const edges = [edge("top", "U1.1", "R1.1", [0, 0], [0, 0]), edge("back", "U1.2", "R1.2", [1, 1], [1, 1]), edge("mixed", "U1.3", "J1.1", [0, 0], [1, 1]), edge("tht", "U1.4", "J1.2", [0, 0], [0, 1])];
  assert.deepEqual(displayedRatsnest(edges, only).map((e) => e.net), ["top", "tht"]);
  assert.equal(displayedRatsnest(edges, view({ visibleLayers: new Set([0]) })).length, 4, "all-layers mode ignores what is shown");
  // An old backend with no layer info is never filtered by it.
  assert.equal(displayedRatsnest([{ net: "x", from: [0, 0], to: [1, 1] }], only).length, 1);
});

test("toggleLocalRatsnestPad flips one pad's side; toggleLocalRatsnestFootprint sets every pad to the opposite of the first pad's state", () => {
  assert.deepEqual(toggleLocalRatsnestPad([], "U1.1"), ["U1.1"]);
  assert.deepEqual(toggleLocalRatsnestPad(["U1.1", "U1.2"], "U1.1"), ["U1.2"]);
  const pads = ["U1.1", "U1.2", "U1.3"];
  // Global on: first pad shows -> all pads off (all differ from the global state).
  assert.deepEqual(toggleLocalRatsnestFootprint([], pads, true).sort(), pads);
  // ... and again: first pad now hidden -> all pads back on.
  assert.deepEqual(toggleLocalRatsnestFootprint(pads, pads, true), []);
});

test("toggleLocalRatsnestFootprint with the global ratsnest off enables every pad, then disables them again", () => {
  const pads = ["U1.1", "U1.2"];
  // Global off: a pad's flag is `flipped.has`. Nothing flipped: first pad's flag is false, so enable = true -> flipped.
  const on = toggleLocalRatsnestFootprint(["R9.1"], pads, false);
  assert.deepEqual(on.sort(), ["R9.1", "U1.1", "U1.2"]);
  assert.deepEqual(toggleLocalRatsnestFootprint(on, pads, false), ["R9.1"], "first pad's flag is true now: disable all of them");
  assert.deepEqual(toggleLocalRatsnestFootprint(["a"], [], true), ["a"], "a footprint with no pads changes nothing");
});

test("setNetsHidden hides and shows nets", () => {
  assert.deepEqual(setNetsHidden([], ["A", "B"], true).sort(), ["A", "B"]);
  assert.deepEqual(setNetsHidden(["A", "B"], ["A"], false), ["B"]);
  assert.deepEqual(setNetsHidden(["A"], ["A"], true), ["A"], "hiding twice is one entry");
});

test("highlightedNets / isHighlighted: the first net plus the rest; a single name or a whole list", () => {
  assert.deepEqual(highlightedNets("A", ["B", "A", "C"]), ["A", "B", "C"]);
  assert.deepEqual(highlightedNets(null, ["B"]), [], "no first net: nothing is highlighted");
  assert.equal(isHighlighted("A", "A"), true);
  assert.equal(isHighlighted("B", "A"), false);
  assert.equal(isHighlighted("B", ["A", "B"]), true);
  assert.equal(isHighlighted(null, ["A"]), false);
  assert.equal(isHighlighted("A", null), false);
});

const board = {
  parts: [
    { ref: "U1", pads: [{ net: "VCC" }, { net: "GND" }, { net: null }, { net: "VCC" }] },
    { ref: "R1", pads: [{ net: "SIG" }] },
    { ref: "H1", pads: [] },
  ],
  routing: { tracks: [{ id: "trk_1", net: "SIG" }], vias: [{ id: "via_1", net: "GND" }], zones: [{ id: "zon_1", net: "" }, { id: "zon_2", net: "VCC" }] },
};

test("netsOfSelection: the nets of the selected connected items in first-seen order, footprints standing for their pads, the unnamed net left out", () => {
  assert.deepEqual(netsOfSelection(["U1"], board), ["VCC", "GND"]);
  assert.deepEqual(netsOfSelection(["R1", "trk_1", "via_1"], board), ["SIG", "GND"]);
  assert.deepEqual(netsOfSelection(["zon_1", "zon_2"], board), ["VCC"], "a zone with no net adds none");
  assert.deepEqual(netsOfSelection(["H1", "nope"], board), []);
  assert.deepEqual(netsOfSelection(new Set(["trk_1"]), { parts: [], routing: null }), []);
});

test("swapHighlight: the current and the last highlight trade places", () => {
  assert.deepEqual(swapHighlight(["A"], ["B", "C"]), { current: ["B", "C"], last: ["A"] });
  assert.deepEqual(swapHighlight([], ["B"]), { current: ["B"], last: [] });
});

const all = ["z1", "z2", "z3"];

test("filledNow: nothing before a fill, everything for 'all', the named ones for a draft fill", () => {
  assert.deepEqual(filledNow(all, null, false), []);
  assert.deepEqual(filledNow(all, null, true), all);
  assert.deepEqual(filledNow(all, ["z2"], true), ["z2"]);
  assert.deepEqual(filledNow(all, ["z2", "gone"], true), ["z2"], "a deleted zone drops out");
});

test("afterFill: a draft fill adds its zones, and filling the last unfilled zone makes it 'all' again", () => {
  assert.deepEqual(afterFill(all, null, false, ["z2"]), ["z2"]);
  assert.deepEqual(afterFill(all, ["z2"], true, ["z1"]), ["z2", "z1"]);
  assert.equal(afterFill(all, ["z1", "z2"], true, ["z3"]), null);
  assert.deepEqual(afterFill(all, null, false, ["z1", "ghost"]), ["z1"], "an unknown id is ignored");
});

test("afterUnfill: the zones left with a fill; empty means nothing is filled", () => {
  assert.deepEqual(afterUnfill(all, null, true, ["z2"]), ["z1", "z3"]);
  assert.deepEqual(afterUnfill(all, ["z1"], true, ["z1"]), []);
  assert.deepEqual(afterUnfill(all, null, false, ["z1"]), [], "nothing was filled to begin with");
});

test("keepFilled: the report is cut to the filled zones, whole for null", () => {
  const report: FillReport = { zones: [{ id: "z1", net: "A", layer: "F.Cu", area_um2: 1, fragments: [] }, { id: "z2", net: "A", layer: "F.Cu", area_um2: 2, fragments: [] }] };
  assert.equal(keepFilled(report, null), report);
  assert.deepEqual(keepFilled(report, ["z2"]).zones.map((z) => z.id), ["z2"]);
  assert.deepEqual(keepFilled(report, []).zones, []);
});

test("selectedCopperZones: only the zones of the selection that are copper (a rule area has no fill)", () => {
  const zones = [{ id: "z1" }, { id: "k1", is_rule_area: true }, { id: "z2" }];
  assert.deepEqual(selectedCopperZones(new Set(["z1", "k1", "R1", "z2"]), zones), ["z1", "z2"]);
  assert.deepEqual(selectedCopperZones([], zones), []);
});

test("zoneOrder / rankPriorities: higher priority first, ties by id; the manager hands out consecutive priorities, the top highest", () => {
  const order = zoneOrder([{ id: "a", priority: 0 }, { id: "b", priority: 5 }, { id: "c", priority: 5 }, { id: "d", priority: 2 }]);
  assert.deepEqual(order, ["c", "b", "d", "a"], "b and c tie at 5: the greater id first");
  assert.deepEqual(rankPriorities(order), { c: 3, b: 2, d: 1, a: 0 });
  assert.deepEqual(rankPriorities([]), {});
});

test("moveZone: up/down trade places with the neighbouring shown row; top/bottom go to the end of the shown rows, the hidden ones staying put", () => {
  const order = ["a", "b", "c", "d", "e"];
  assert.deepEqual(moveZone(order, order, "c", "up"), ["a", "c", "b", "d", "e"]);
  assert.deepEqual(moveZone(order, order, "c", "down"), ["a", "b", "d", "c", "e"]);
  assert.deepEqual(moveZone(order, order, "c", "top"), ["c", "a", "b", "d", "e"]);
  assert.deepEqual(moveZone(order, order, "c", "bottom"), ["a", "b", "d", "e", "c"]);
  // Nothing to do at the ends (and a zone that is not shown does not move).
  assert.deepEqual(moveZone(order, order, "a", "up"), order);
  assert.deepEqual(moveZone(order, order, "e", "down"), order);
  assert.deepEqual(moveZone(order, order, "a", "top"), order);
  assert.deepEqual(moveZone(order, ["a", "c"], "b", "up"), order);
  // A filter showing a, c, e: moving e up trades with c, b and d (hidden) stay where they are.
  assert.deepEqual(moveZone(order, ["a", "c", "e"], "e", "up"), ["a", "b", "e", "d", "c"]);
  assert.deepEqual(moveZone(order, ["a", "c", "e"], "e", "top"), ["e", "b", "a", "d", "c"]);
  assert.deepEqual(moveZone(order, ["a", "c", "e"], "a", "bottom"), ["c", "b", "e", "d", "a"]);
});

test("filterZones: by name and/or net text, case-insensitive, and by layer", () => {
  const zones = [{ id: "zon_1", net: "GND", layer: "F.Cu" }, { id: "zon_2", net: "VCC", layer: "F.Cu" }, { id: "pwr", net: "gnd", layer: "B.Cu" }];
  assert.equal(filterZones(zones, "", true, true, null).length, 3);
  assert.deepEqual(filterZones(zones, "GND", true, true, null).map((z) => z.id), ["zon_1", "pwr"]);
  assert.deepEqual(filterZones(zones, "gnd", false, true, null).map((z) => z.id), ["zon_1", "pwr"], "net only");
  assert.deepEqual(filterZones(zones, "zon", true, false, null).map((z) => z.id), ["zon_1", "zon_2"], "name only");
  assert.deepEqual(filterZones(zones, "zon", false, true, null), [], "the name box is off");
  assert.deepEqual(filterZones(zones, "gnd", true, true, "B.Cu").map((z) => z.id), ["pwr"]);
  assert.deepEqual(filterZones(zones, "  ", true, true, "F.Cu").map((z) => z.id), ["zon_1", "zon_2"], "blank text filters nothing");
});

test("connectedTrackWidth: a route that starts at a track's end takes that track's width; a pad or via start leaves the current width", () => {
  const tracks = [{ id: "trk_1", width: 400 }, { id: "trk_2", width: 250 }];
  assert.equal(connectedTrackWidth("track trk_1", tracks), 400);
  assert.equal(connectedTrackWidth("track trk_2", tracks), 250);
  assert.equal(connectedTrackWidth("track gone", tracks), null);
  assert.equal(connectedTrackWidth("U1.3", tracks), null);
  assert.equal(connectedTrackWidth("via via_1", tracks), null);
  assert.equal(connectedTrackWidth(undefined, tracks), null);
});

test("duplicatedZoneOutline: moved 1 mm each way when the copy stays on the layer, as it was when it goes to another", () => {
  const outline: [number, number][] = [[0, 0], [10_000, 0], [10_000, 10_000]];
  assert.deepEqual(duplicatedZoneOutline(outline, true), [[1000, 1000], [11_000, 1000], [11_000, 11_000]]);
  assert.deepEqual(duplicatedZoneOutline(outline, false), outline);
});

test("flipLocalX: the mirror about the canvas middle is its own inverse and leaves the middle where it is", () => {
  assert.equal(flipLocalX(false, 800, 100), 100);
  assert.equal(flipLocalX(true, 800, 100), 700);
  assert.equal(flipLocalX(true, 800, 400), 400);
  assert.equal(flipLocalX(true, 800, flipLocalX(true, 800, 123.5)), 123.5);
});

test("panDeltaX: a drag to the right moves a mirrored view's origin to the left", () => {
  assert.equal(panDeltaX(false, 30), 30);
  assert.equal(panDeltaX(true, 30), -30);
  assert.equal(panDeltaX(true, 0), 0);
});

test("flipPan: a pan made without the mirror in mind goes the other way along x only when the view is flipped", () => {
  const before = { x: 100, y: 50, scale: 0.01 };
  const after = { x: 130, y: 20, scale: 0.01 };
  assert.deepEqual(flipPan(false, before, after), after);
  assert.deepEqual(flipPan(true, before, after), { x: 70, y: 20, scale: 0.01 });
});
