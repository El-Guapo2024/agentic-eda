import { test } from "node:test";
import assert from "node:assert/strict";
import { dragEndsOnActivate, dragHighlight, dragStateFromPreview, inlineDragBegin, inlineDragEnd, routerStartHighlight, setRouterStartHighlight, type DragDrawState } from "./dragTool";
import type { DragPreview } from "../api/types";

function baseDraw(overrides: Partial<DragDrawState> = {}): DragDrawState {
  return { kind: "drag", dragKind: "corner", net: "SIG", layer: "F.Cu", width: 200, pts: [[0, 0]], ...overrides };
}

function basePreview(overrides: Partial<DragPreview> = {}): DragPreview {
  return { ok: true, colliding: false, pts: [[0, 0]], displaced: [], displaced_vias: [], fanout: [], ...overrides };
}

test("dragStateFromPreview copies the server's pts/colliding into the draw state", () => {
  const current = baseDraw();
  const preview = basePreview({ pts: [[0, 0], [1000, 1000]], colliding: true });
  const next = dragStateFromPreview(current, preview);
  assert.deepEqual(next.pts, preview.pts);
  assert.equal(next.colliding, true);
  // Session-local fields the server never echoes back must survive.
  assert.equal(next.net, "SIG");
  assert.equal(next.dragKind, "corner");
  assert.equal(next.layer, "F.Cu");
  assert.equal(next.width, 200);
});

test("dragStateFromPreview carries displaced lines/vias and via fanout through untouched", () => {
  const current = baseDraw({ dragKind: "via", viaDiameter: 600 });
  const preview = basePreview({
    pts: [[2000, 2000]],
    displaced: [{ source_track: "trk_abc", layer: "F.Cu", pts: [[500, -500], [500, 500]] }],
    displaced_vias: [{ source_via: "via_xyz", x: 3000, y: 3000 }],
    fanout: [{ layer: "F.Cu", width: 250, pts: [[0, 0], [2000, 2000]] }],
  });
  const next = dragStateFromPreview(current, preview);
  assert.deepEqual(next.displaced, preview.displaced);
  assert.deepEqual(next.displacedVias, preview.displaced_vias);
  assert.deepEqual(next.fanout, preview.fanout);
  // The via's own size, captured at drag-start, is untouched by a preview reply.
  assert.equal(next.viaDiameter, 600);
});

test("a position the drag cannot resolve shows the last valid line, flagged; a drag that only marks obstacles says so", () => {
  const slid = baseDraw({ dragKind: "segment", pts: [[0, 0], [0, 800], [4000, 800], [4000, 0]] });
  const restored = dragStateFromPreview(slid, basePreview({ drag: "segment", pts: [[0, 0], [0, 500], [4000, 500], [4000, 0]], restored: true }));
  assert.equal(restored.restored, true);
  assert.equal(restored.forcedMark, false);
  assert.equal(restored.dragKind, "segment", "what was grabbed is the session's, captured at drag start");
  const marking = dragStateFromPreview(slid, basePreview({ colliding: true, forced_mark_obstacles: true }));
  assert.equal(marking.forcedMark, true);
  assert.equal(marking.restored, false);
  // a legal position clears both flags again
  const legal = dragStateFromPreview({ ...marking, restored: true }, basePreview());
  assert.equal(legal.restored, false);
});

// ------------------------------------------------------------ D on a selection: COMPONENT_DRAGGER and MULTI_DRAGGER

import { findGroupDragTarget } from "./dragTool";
import type { BoardState, Pad, Part } from "../api/types";

function board(partial: Partial<BoardState>): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...partial } as BoardState;
}
const pad = (num: string, x: number, y: number): Pad => ({ num, net: null, x, y, w: 600, h: 600, round: false, th: false });
const part = (ref: string, at: [number, number]): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1000, at[1] - 500, at[0] + 1000, at[1] + 500], pads: [pad("1", at[0], at[1])] }) as Part;
const track = (id: string, arc = false) => ({ id, net: "GND", layer: "F.Cu", width: 200, pts: [[0, 0], [1000, 0]] as [number, number][], arc_mid: arc ? ([500, 200] as [number, number]) : null });
const routing = (tracks: ReturnType<typeof track>[]) => ({ tracks, vias: [{ id: "v1", net: "GND", x: 0, y: 0, d: 600, drill: 300 }], zones: [], track_width_presets: [], via_presets: [], teardrop_settings: {} }) as unknown as BoardState["routing"];

test("D on footprints: a selection that holds one keeps its footprints only, the footprint (or pad) under the cursor drags it, a locked one stays", () => {
  const b = board({ parts: [part("U1", [0, 0]), part("U2", [5000, 0]), part("U3", [9000, 0])], routing: routing([track("t1"), track("t2")]), locked: ["U3"] });
  // "Remove non-footprints so box-selection will drag footprints"
  assert.deepEqual(findGroupDragTarget(b, ["t1", "U1", "U2"], null, false).target, { kind: "footprints", refs: ["U1", "U2"] });
  // the hover selection: the footprint, or the pad that stands for it (`FilterCollectorForFreePads`)
  assert.deepEqual(findGroupDragTarget(b, [], "U2", false).target, { kind: "footprints", refs: ["U2"] });
  assert.deepEqual(findGroupDragTarget(b, [], "U1.1", false).target, { kind: "footprints", refs: ["U1"] });
  // `FilterCollectorForLockedItems`
  assert.deepEqual(findGroupDragTarget(b, ["U3"], null, false), { target: null, lockedOut: true });
  assert.deepEqual(findGroupDragTarget(b, ["U1", "U3"], null, false), { target: { kind: "footprints", refs: ["U1"] }, lockedOut: true });
  // "Footprints cannot be dragged freely" -- but one footprint is a `DraggableItems` item, which `G` drags as `D` does
  assert.equal(findGroupDragTarget(b, ["U1", "U2"], null, true).target, null);
  assert.deepEqual(findGroupDragTarget(b, ["U1"], null, true).target, { kind: "footprints", refs: ["U1"] });
});

test("D on tracks: two or more, all of them tracks (`CountType( PCB_TRACE_T ) == Size()`), drag together; one track, an arc or a via among them does not", () => {
  const b = board({ parts: [part("U1", [0, 0])], routing: routing([track("t1"), track("t2"), track("t3"), track("a1", true)]) });
  assert.deepEqual(findGroupDragTarget(b, ["t1", "t2", "t3"], null, false).target, { kind: "tracks", ids: ["t1", "t2", "t3"] });
  assert.equal(findGroupDragTarget(b, ["t1"], null, false).target, null, "one track is the single drag");
  assert.equal(findGroupDragTarget(b, ["t1", "a1"], null, false).target, null, "an arc track is no PCB_TRACE_T");
  assert.equal(findGroupDragTarget(b, ["t1", "v1"], null, false).target, null, "a via either");
  assert.equal(findGroupDragTarget(b, [], "t1", false).target, null, "the hover selection is the track under the cursor: the single drag");
});

test("a component drag's preview keeps its footprints and start, takes the offset and marks the cursor as moved", () => {
  const current = baseDraw({ dragKind: "component", footprints: ["U1"], start: [0, 0], grabbed: [100, 50], offset: [0, 0], moved: false, pts: [] });
  const next = dragStateFromPreview(current, basePreview({ drag: "component", pts: [], offset: [1000, -2000], fanout: [{ layer: "F.Cu", width: 200, pts: [[1000, -2000], [5000, 0]] }] }));
  assert.deepEqual(next.offset, [1000, -2000]);
  assert.deepEqual(next.footprints, ["U1"]);
  assert.deepEqual(next.start, [0, 0]);
  assert.equal(next.moved, true);
  assert.equal(next.fanout?.length, 1);
});

// UI-LEFT-2 RR1: `TOOL_BASE::highlightNets( true, nets )` -- the drag's net is shown; at the end the highlight goes back when it already held that net
// ("we assume they want to keep them highlighted after routing"), else off; no net is no call.
test("dragHighlight shows the dragged net and puts back what was highlighted only when it held that net", () => {
  assert.deepEqual(dragHighlight([], ["SIG"]), { show: ["SIG"], restore: [] });
  assert.deepEqual(dragHighlight(["GND"], ["SIG"]), { show: ["SIG"], restore: [] }, "another net's highlight goes when the drag ends");
  assert.deepEqual(dragHighlight(["GND", "SIG"], ["SIG"]), { show: ["SIG"], restore: ["GND", "SIG"] }, "kept when it held the dragged net");
  assert.equal(dragHighlight(["GND"], []), null);
  assert.equal(dragHighlight(["GND"], [""]), null, "a track of no net");
});

// `ROUTER_TOOL::InlineDrag`: the selection tool's drag clears the selection at its start and Esc puts it back (`restoreSelection( selection )`);
// a committed multi-track drag selects its leaders' tracks; the router tool's own drag (`performDragging`) leaves the selection alone.
test("an inline drag clears the selection, Esc puts it back, a commit selects the leaders, the router tool's drag leaves it", () => {
  const context = { selection: ["t1", "t2"], highlighted: ["GND"] };
  const begun = inlineDragBegin(baseDraw({ dragKind: "multi" }), context, true, "SIG");
  assert.equal(begun.clearSelection, true);
  assert.deepEqual(begun.highlight, ["SIG"]);
  assert.deepEqual(begun.draw.restoreSelection, ["t1", "t2"]);
  assert.deepEqual(begun.draw.restoreHighlight, []);
  assert.deepEqual(inlineDragEnd(begun.draw, { cancelled: true }), { highlight: [], selection: ["t1", "t2"] });
  assert.deepEqual(inlineDragEnd(begun.draw, { cancelled: false, select: ["t9", "t10"] }), { highlight: [], selection: ["t9", "t10"] });
  assert.deepEqual(inlineDragEnd(begun.draw, { cancelled: false }), { highlight: [], selection: null }, "a single drag's commit: the selection stays clear");
  const router = inlineDragBegin(baseDraw(), { selection: [], highlighted: [] }, false, "SIG");
  assert.equal(router.clearSelection, false);
  assert.equal(router.draw.restoreSelection, undefined);
  assert.deepEqual(inlineDragEnd(router.draw, { cancelled: true }), { highlight: [], selection: null });
  // no context (a caller that knows nothing of the editor): nothing changes
  assert.deepEqual(inlineDragBegin(baseDraw(), undefined, true, "SIG"), { draw: baseDraw(), clearSelection: false, highlight: null });
  assert.deepEqual(inlineDragEnd(null, { cancelled: true }), { highlight: null, selection: null });
});

// UI-LEFT-3 R3-10: `highlightNets( false )` at the end of every drag puts back the router tool's `m_startHighlightNetcodes` -- what the last drag with a
// net left there, an empty set turning the highlight off -- a drag of no net included, which never sets it.
test("a drag of no net puts back what the last drag with a net left, and the user's highlight goes when that is nothing", () => {
  setRouterStartHighlight([]);
  const none = inlineDragBegin(baseDraw({ dragKind: "multi" }), { selection: ["t1"], highlighted: ["GND"] }, true, null);
  assert.equal(none.highlight, null, "no net: no highlightNets( true )");
  assert.deepEqual(inlineDragEnd(none.draw, { cancelled: false }).highlight, [], "SetHighlight( {}, false ): off");
  const kept = inlineDragBegin(baseDraw(), { selection: [], highlighted: ["GND", "SIG"] }, true, "SIG");
  assert.deepEqual(kept.draw.restoreHighlight, ["GND", "SIG"]);
  assert.deepEqual(routerStartHighlight(), ["GND", "SIG"]);
  const later = inlineDragBegin(baseDraw({ dragKind: "multi" }), { selection: [], highlighted: [] }, true, null);
  assert.deepEqual(inlineDragEnd(later.draw, { cancelled: true }).highlight, ["GND", "SIG"], "the last drag's set comes back");
  setRouterStartHighlight([]);
});

// UI-LEFT-3 R3-10: `InlineDrag`'s click ends the drag whatever `FixRoute` says (`inline`); another tool's activation (`IsActivate()`) cancels it as
// Esc does.
test("an inline drag ends on a refused click and on another tool's activation", () => {
  setRouterStartHighlight([]);
  assert.equal(inlineDragBegin(baseDraw(), { selection: ["t1"], highlighted: [] }, true, "SIG").draw.inline, true);
  assert.equal(inlineDragBegin(baseDraw(), { selection: [], highlighted: [] }, false, "SIG").draw.inline, undefined, "the router tool's drag goes on");
  assert.equal(dragEndsOnActivate("drag", "route", "drag"), true);
  assert.equal(dragEndsOnActivate("drag", "select", null), false, "a drag that ended");
  assert.equal(dragEndsOnActivate("select", "drag", "drag"), false, "the drag starting");
  setRouterStartHighlight([]);
});
