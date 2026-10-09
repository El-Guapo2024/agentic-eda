import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { boardHasItems, summarizePcbSelection } from "./pcbSelectionSummary";
import { menuActions, PCB_MENU_ACTIONS as A, pcbSelectionMenu } from "./pcbContextMenu";

function board(partial: Partial<BoardState>): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...partial } as BoardState;
}
const pad = (num: string, x: number, y: number, net: string | null = null): Pad => ({ num, net, x, y, w: 600, h: 600, round: false, th: false });
const part = (ref: string, at: [number, number], pads: Pad[] = []): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1000, at[1] - 500, at[0] + 1000, at[1] + 500], pads }) as Part;
const track = (id: string, net = "GND", arc = false) => ({ id, net, layer: "F.Cu", width: 200, pts: [[0, 0], [1000, 0]] as [number, number][], ...(arc ? { arc_mid: [500, 100] as [number, number] } : {}) });
const zone = (id: string, over: Record<string, unknown> = {}) => ({ id, net: "GND", layer: "F.Cu", teardrop: false, is_rule_area: false, priority: 0, outline: [[0, 0], [5000, 0], [5000, 5000], [0, 5000]] as [number, number][], ...over });
const routing = (tracks: ReturnType<typeof track>[], zones: ReturnType<typeof zone>[] = [], vias: unknown[] = []) => ({ tracks, vias, zones, track_width_presets: [], via_presets: [], teardrop_settings: {} }) as unknown as BoardState["routing"];
const drawings = (extra: Record<string, unknown> = {}) => ({ shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {}, ...extra }) as unknown as NonNullable<BoardState["drawings"]>;
const shape = (id: string, kind: string, layer = "F.SilkS") => ({ id, kind, layer, stroke_width: 150, filled: false, start: [0, 0], end: [1000, 1000], center: [0, 0], mid: [500, 500], pts: [[0, 0], [1000, 0], [1000, 1000]], c1: [0, 0], c2: [0, 0] });

test("every kind of item is counted under its KiCad type, an arc track apart from a straight one, shapes by shape and copper", () => {
  const b = board({
    parts: [part("U1", [0, 0], [pad("1", -500, 0, "VCC")])],
    routing: routing([track("t1"), track("t2", "GND", true)], [zone("z1")], [{ id: "v1", net: "GND", x: 0, y: 0, d: 600, drill: 300, from_layer: "F.Cu", to_layer: "B.Cu" }]),
    drawings: drawings({
      shapes: [shape("s1", "segment"), shape("s2", "rect", "F.Cu"), shape("s3", "arc"), shape("s4", "polygon"), shape("s5", "circle"), shape("s6", "bezier")],
      texts: [{ id: "x1", content: "T", x: 0, y: 0, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 150, justify: "center", mirror: false }],
      dimensions: [{ id: "d1", layer: "Dwgs.User", start: [0, 0], end: [1000, 0], lines: [], text: "1", text_at: [0, 0], computed_text_angle: 0, text_size_um: 1000, stroke_width: 150, kind: "aligned" }],
      groups: [{ id: "g1", name: "", member_ids: ["t1", "t2"] }],
    }),
  });
  const s = summarizePcbSelection(b, ["U1", "U1.1", "t1", "t2", "v1", "z1", "s1", "s2", "s3", "s4", "s5", "s6", "x1", "d1", "g1", "nothing"]);
  assert.equal(s.total, 15, "an id the board does not have counts as nothing");
  assert.deepEqual([s.footprints, s.pads, s.tracks, s.arcTracks, s.vias, s.zones, s.copperZones, s.texts, s.dimensions, s.groups], [1, 1, 1, 1, 1, 1, 1, 1, 1, 1]);
  assert.deepEqual(s.shapes, { segment: 1, rect: 1, circle: 1, arc: 1, polygon: 1, bezier: 1 });
  assert.equal(s.copperShapes, 1, "only the rectangle is on a copper layer");
  assert.ok(s.hasNet);
});

test("locked items: an item's own lock, or its group's, counts it locked", () => {
  const b = board({
    parts: [part("U1", [0, 0]), part("U2", [9000, 0]), part("U3", [18000, 0])],
    drawings: drawings({ groups: [{ id: "g", name: "", member_ids: ["U2", "U3"] }] }),
    locked: ["U1", "g"],
  });
  const s = summarizePcbSelection(b, ["U1", "U2", "U3"]);
  assert.deepEqual([s.locked, s.unlocked], [3, 0]);
  assert.deepEqual(menuActions(pcbSelectionMenuLocking(s)), [A.unlock, A.toggleLock]);
  const free = summarizePcbSelection(board({ parts: [part("U1", [0, 0])] }), ["U1"]);
  assert.deepEqual([free.locked, free.unlocked], [0, 1]);
});

function pcbSelectionMenuLocking(s: ReturnType<typeof summarizePcbSelection>) {
  const lock = pcbSelectionMenu(s).find((n) => n.type === "submenu" && n.label === "Locking");
  return lock && lock.type === "submenu" ? lock.items : [];
}

test("the group tool's facts: one group and a loose item, a member, two groups", () => {
  const b = board({
    parts: [part("U1", [0, 0]), part("U2", [9000, 0]), part("U3", [18000, 0]), part("U4", [27000, 0]), part("U5", [36000, 0])],
    drawings: drawings({
      groups: [
        { id: "g1", name: "", member_ids: ["U1", "U2"] },
        { id: "g2", name: "", member_ids: ["U3", "U4"] },
      ],
    }),
  });
  assert.deepEqual(summarizePcbSelection(b, ["g1", "U5"]).group, { hasGroup: true, onlyOneGroup: true, hasUngroupedItems: true, hasMember: false });
  assert.deepEqual(summarizePcbSelection(b, ["g1", "g2"]).group, { hasGroup: true, onlyOneGroup: false, hasUngroupedItems: false, hasMember: false });
  assert.deepEqual(summarizePcbSelection(b, ["U1", "U5"]).group, { hasGroup: false, onlyOneGroup: false, hasUngroupedItems: true, hasMember: true });
  assert.deepEqual(summarizePcbSelection(b, ["U5"]).group, { hasGroup: false, onlyOneGroup: false, hasUngroupedItems: true, hasMember: false });
});

test("the group tool's facts see a nested group as a member, and its items as members too", () => {
  const b = board({
    parts: [part("U1", [0, 0]), part("U2", [9000, 0]), part("U3", [18000, 0])],
    drawings: drawings({ groups: [{ id: "outer", name: "", member_ids: ["inner", "U3"] }, { id: "inner", name: "", member_ids: ["U1", "U2"] }] }),
  });
  const s = summarizePcbSelection(b, ["inner"]);
  assert.deepEqual(s.group, { hasGroup: true, onlyOneGroup: true, hasUngroupedItems: false, hasMember: true }, "inner is a group, and a member of outer");
});

test("a pad stands for its footprint when the group tool asks whether it is grouped", () => {
  const b = board({ parts: [part("U1", [0, 0], [pad("1", 0, 0)]), part("U2", [9000, 0])], drawings: drawings({ groups: [{ id: "g", name: "", member_ids: ["U1", "U2"] }] }) });
  const s = summarizePcbSelection(b, ["U1.1"]);
  assert.equal(s.pads, 1);
  assert.equal(s.group.hasUngroupedItems, false, "its footprint is in a group");
});

test("the point editor's facts: a zone or polygon has corners to edit, a teardrop area does not; a segment or an arc can take a corner", () => {
  const b = board({
    routing: routing([], [zone("z1"), zone("z2", { teardrop: true })]),
    drawings: drawings({ shapes: [shape("poly", "polygon"), shape("seg", "segment"), shape("arc", "arc"), shape("rect", "rect")] }),
  });
  const of = (id: string) => summarizePcbSelection(b, [id]);
  assert.deepEqual([of("z1").editableCorners, of("z1").canAddCorner, of("z1").canChamferCorner], [true, true, true]);
  assert.deepEqual([of("z2").editableCorners, of("z2").canAddCorner, of("z2").canChamferCorner], [false, true, true], "a teardrop area: KiCad's itemHasEditableCorners says no, CanAddCorner says zone");
  assert.deepEqual([of("poly").editableCorners, of("poly").canAddCorner, of("poly").canChamferCorner], [true, true, true]);
  assert.deepEqual([of("seg").editableCorners, of("seg").canAddCorner, of("seg").canChamferCorner], [false, true, false]);
  assert.deepEqual([of("arc").editableCorners, of("arc").canAddCorner, of("arc").canChamferCorner], [false, true, false]);
  assert.deepEqual([of("rect").editableCorners, of("rect").canAddCorner, of("rect").canChamferCorner], [false, false, false]);
  assert.equal(summarizePcbSelection(b, ["z1", "poly"]).editableCorners, false, "only for a single item");
});

test("Zone Priority: raise when a zone it overlaps is above it, lower when one is below; rule areas and other layers do not count", () => {
  const b = board({
    routing: routing(
      [],
      [
        zone("mid", { priority: 5 }),
        zone("above", { priority: 8 }),
        zone("below", { priority: 2 }),
        zone("elsewhere", { priority: 9, layer: "B.Cu" }),
        zone("rule", { priority: 9, is_rule_area: true }),
        zone("far", { priority: 1, outline: [[50_000, 50_000], [51_000, 50_000], [51_000, 51_000]] }),
      ]
    ),
  });
  const mid = summarizePcbSelection(b, ["mid"]);
  assert.deepEqual([mid.zoneCanRaise, mid.zoneCanLower], [true, true]);
  const above = summarizePcbSelection(b, ["above"]);
  assert.deepEqual([above.zoneCanRaise, above.zoneCanLower], [false, true]);
  const far = summarizePcbSelection(b, ["far"]);
  assert.deepEqual([far.zoneCanRaise, far.zoneCanLower], [false, false], "overlaps nothing");
  const lone = summarizePcbSelection(board({ routing: routing([], [zone("z", { priority: 3 })]) }), ["z"]);
  assert.deepEqual([lone.zoneCanRaise, lone.zoneCanLower], [false, false]);
});

test("Net Inspection Tools' enable state: a track, via, pad or zone with a net carries one; a track on no net does not", () => {
  assert.ok(summarizePcbSelection(board({ routing: routing([track("t1", "GND")]) }), ["t1"]).hasNet);
  assert.ok(!summarizePcbSelection(board({ routing: routing([track("t1", "")]) }), ["t1"]).hasNet);
  assert.ok(summarizePcbSelection(board({ parts: [part("U1", [0, 0], [pad("1", 0, 0, "VCC")])] }), ["U1.1"]).hasNet);
  assert.ok(!summarizePcbSelection(board({ parts: [part("U1", [0, 0], [pad("1", 0, 0, "VCC")])] }), ["U1"]).hasNet, "a footprint is not a connected item");
});

test("Convert's conditions come from the whole selection", () => {
  const b = board({ drawings: drawings({ shapes: [shape("r1", "rect"), shape("r2", "rect")] }), parts: [part("U1", [0, 0])] });
  const rects = summarizePcbSelection(b, ["r1", "r2"]);
  assert.ok(rects.convert.poly && rects.convert.lines && rects.convert.outset);
  const fp = summarizePcbSelection(b, ["U1"]);
  assert.ok(!fp.convert.poly && !fp.convert.lines);
});

test("a board with something on it is not empty", () => {
  assert.ok(!boardHasItems(board({})));
  assert.ok(boardHasItems(board({ parts: [part("U1", [0, 0])] })));
  assert.ok(!boardHasItems(board({ parts: [{ ...part("U1", [0, 0]), placed: false }] })), "an unplaced part is not on the board");
  assert.ok(boardHasItems(board({ routing: routing([track("t1")]) })));
  assert.ok(boardHasItems(board({ drawings: drawings({ texts: [{ id: "x" }] }) })));
});
