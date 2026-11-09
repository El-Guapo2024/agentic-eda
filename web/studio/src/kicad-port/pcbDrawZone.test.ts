import { test } from "node:test";
import assert from "node:assert/strict";
import { leaderModeOf, newZoneInitial, newZoneNet, stickySettings, uniqueZonePriority, zoneClick, zoneCommitPlan, zoneDefaultsCmd, zoneFinish, zoneFirstCorner, zoneFromExisting, zoneLeader, zoneLoop } from "./pcbDrawZone";
import type { BoardState } from "../api/types";

test("a click on the first corner closes the zone or polygon (NewPointClosesOutline, round 4 C-4)", () => {
  const pts: [number, number][] = [[0, 0], [10, 0], [10, 10]];
  assert.deepEqual(zoneClick(pts, [0, 0], "direct"), { finish: true });
  assert.deepEqual(zoneClick(pts, [0, 10], "direct"), { finish: false, pts: [...pts, [0, 10]] });
  assert.deepEqual(zoneClick([], [5, 5], "deg45"), { finish: false, pts: [[5, 5]] }, "the first corner");
});

test("in the 45-degree line mode a click locks in the leader's bend and end (AddPoint)", () => {
  // From (0,0), the first leg is GetVectorSnapped45: (10,3) goes onto the axis (|x| > 2|y|).
  assert.deepEqual(zoneClick([[0, 0]], [10, 3], "deg45"), { finish: false, pts: [[0, 0], [10, 0]] });
  // From (0,0)-(10,0), a click at (20,4): a straight run then a 45-degree bend into the cursor.
  const r = zoneClick([[0, 0], [10, 0]], [20, 4], "deg45");
  assert.deepEqual(r, { finish: false, pts: [[0, 0], [10, 0], [16, 0], [20, 4]] });
  assert.deepEqual(zoneLeader([[0, 0], [10, 0]], [20, 4], "deg45"), [[10, 0], [16, 0], [20, 4]]);
  assert.deepEqual(zoneClick([[0, 0], [10, 0]], [20, 4], "deg90"), { finish: false, pts: [[0, 0], [10, 0], [20, 0], [20, 4]] });
  assert.deepEqual(zoneClick([[0, 0], [10, 0]], [20, 4], "direct"), { finish: false, pts: [[0, 0], [10, 0], [20, 4]] });
});

test("Ctrl draws free-angle whatever the line mode", () => {
  assert.equal(leaderModeOf("45", true), "direct");
  assert.equal(leaderModeOf("45", false), "deg45");
  assert.equal(leaderModeOf("90", false), "deg90");
  assert.equal(leaderModeOf("direct", false), "direct");
});

test("finishing a zone in the 45-degree mode adds the loop's bend (ZONE_CREATE_HELPER::OnComplete, round 5 A5-5)", () => {
  // (0,0), (10,0), (10,10), then a click at (3,7): from (10,10) the leader runs straight to (6,10) and 45 degrees on to (3,7), both locked in.
  let pts: [number, number][] = [[0, 0]];
  for (const at of [[10, 0], [10, 10], [3, 7]] as [number, number][]) {
    const c = zoneClick(pts, at, "deg45");
    assert.equal(c.finish, false);
    if (!c.finish) pts = c.pts;
  }
  assert.deepEqual(pts, [[0, 0], [10, 0], [10, 10], [6, 10], [3, 7]]);
  // A double-click at (3,7): the closing edge to (0,0) is bent onto 45/90 legs, as the preview's loop shows it.
  const loop = zoneLoop(pts, [3, 7], "deg45");
  assert.ok(loop.length === 3, `the loop has a bend: ${JSON.stringify(loop)}`);
  const out = zoneFinish(pts, [3, 7], "deg45")!;
  // The loop goes 45 degrees to (0,4) and down to (0,0); (3,7) is then on the line (6,10)-(0,4), and `Simplify` drops it.
  assert.deepEqual(out, [[0, 0], [10, 0], [10, 10], [6, 10], [0, 4]]);
  assert.ok(out.some((p) => p[0] === loop[1]![0] && p[1] === loop[1]![1]), "the loop's corner is in the outline");
  // Free mode: the corners as clicked, the straight closing edge.
  assert.deepEqual(zoneFinish(pts, [3, 7], "direct"), pts);
});

test("finishing simplifies the outline: collinear and repeated corners go, the start too when it is on its neighbours' line", () => {
  assert.deepEqual(zoneFinish([[0, 0], [5, 0], [10, 0], [10, 10]], [10, 10], "direct"), [[0, 0], [10, 0], [10, 10]]);
  assert.deepEqual(zoneFinish([[0, 0], [10, 0], [10, 0], [10, 10]], [10, 10], "direct"), [[0, 0], [10, 0], [10, 10]]);
  assert.deepEqual(zoneFinish([[5, 0], [10, 0], [10, 10], [0, 10], [0, 0]], [0, 0], "direct"), [[10, 0], [10, 10], [0, 10], [0, 0]]);
  assert.equal(zoneFinish([[0, 0], [10, 0]], [10, 0], "direct"), null, "fewer than three corners: no zone");
});


const defaults = { clearance: 500_000, min_thickness: 250_000, thermal_gap: 500_000, thermal_spoke_width: 500_000, pad_connection: "Thermal", priority: 0, island_removal_mode: "Always", min_island_area: 0, fill_mode: "Polygons", hatch_thickness: 1, hatch_gap: 1, hatch_orientation_mdeg: 0, hatch_smoothing_level: 0, hatch_smoothing_value: 0.1, hatch_hole_min_area: 0.15, hatch_border_algorithm: 1, is_rule_area: false, keepout_tracks: false, keepout_vias: false, keepout_pads: false, keepout_copper_pour: false, keepout_footprints: false } as const;
const zone = (id: string, priority: number, more: Record<string, unknown> = {}) => ({ id, net: "GND", layer: "F.Cu", outline: [], teardrop: false, ...defaults, priority, ...more });
const board = {
  parts: [{ ref: "R1", placed: true, pads: [{ num: "1", net: "+3V3" }] }],
  routing: { tracks: [{ id: "t1", net: "SIG", layer: "F.Cu", pts: [], width: 1 }], vias: [], zones: [zone("z0", 0), zone("z1", 1), zone("z3", 3), zone("ra", 2, { is_rule_area: true }), zone("td", 2, { teardrop: true }), zone("silk", 2, { layer: "F.SilkS" })] },
} as unknown as BoardState;

test("setUniquePriority: the first priority no copper pour holds (rule areas, teardrops and non-copper zones do not count)", () => {
  assert.equal(uniqueZonePriority(board.routing!.zones), 2);
  assert.equal(uniqueZonePriority([]), 0);
});

test("createNewZone's net: the highlighted net, else the first selected connected item's, else none", () => {
  assert.equal(newZoneNet(board, ["VBUS"], ["t1"]), "VBUS");
  assert.equal(newZoneNet(board, [], ["t1", "R1.1"]), "SIG");
  assert.equal(newZoneNet(board, [], ["R1.1"]), "+3V3", "a pad");
  assert.equal(newZoneNet(board, [], ["R1"]), "", "a footprint is no connected item");
  assert.equal(newZoneNet(board, [], []), "", "<no net>");
});

test("createNewZone's settings: the drawing layer alone, a rule area for Add Rule Area, the unique priority on copper only", () => {
  const z = newZoneInitial(board, { layer: "B.Cu", ruleArea: false, highlighted: [], selection: ["t1"], defaults });
  assert.deepEqual([z.net, z.layers, z.name, z.locked, z.settings.priority, z.settings.is_rule_area], ["SIG", ["B.Cu"], "", false, 2, false]);
  const ra = newZoneInitial(board, { layer: "F.Cu", ruleArea: true, highlighted: [], selection: [], defaults });
  assert.equal(ra.settings.is_rule_area, true);
  // `ZONE_SETTINGS::ZONE_SETTINGS()`: a new rule area keeps out tracks, vias and pads, not zone fills or footprints (UI-LEFT P29)
  assert.deepEqual([ra.settings.keepout_tracks, ra.settings.keepout_vias, ra.settings.keepout_pads, ra.settings.keepout_copper_pour, ra.settings.keepout_footprints], [true, true, true, false, false]);
  assert.equal(newZoneInitial(board, { layer: "F.Cu", ruleArea: true, highlighted: ["GND"], selection: [], defaults }).net, "", "a rule area takes no net");
  assert.equal(ra.settings.priority, 2, "a rule area on copper gets one too (`setUniquePriority` runs on the layers)");
  assert.equal(newZoneInitial(board, { layer: "F.SilkS", ruleArea: false, highlighted: [], selection: [], defaults }).settings.priority, 0, "not on copper: the default");
});

test("the zone tool's order: Add Filled Zone and Add Rule Area ask first, the cutout, similar zone and via stitching draw at once", () => {
  assert.equal(zoneFirstCorner(null), "ask");
  assert.equal(zoneFirstCorner({ mode: "cutout", sourceId: "z0" }), "draw");
  assert.equal(zoneFirstCorner({ mode: "similar", sourceId: "z0" }), "draw");
  assert.equal(zoneFirstCorner({ mode: "stitch", sourceId: "" }), "draw");
});

test("commitZone: the drawn zone takes the dialog's settings; a cutout or similar zone its source's; no source, nothing", () => {
  const settings = newZoneInitial(board, { layer: "F.Cu", ruleArea: false, highlighted: ["GND"], selection: [], defaults });
  assert.deepEqual(zoneCommitPlan(null, false, settings), { kind: "add", zone: settings });
  assert.deepEqual(zoneCommitPlan(null, false, undefined), { kind: "ask" });
  assert.deepEqual(zoneCommitPlan({ mode: "cutout", sourceId: "z0" }, true, settings), { kind: "cutout", sourceId: "z0" });
  assert.deepEqual(zoneCommitPlan({ mode: "similar", sourceId: "z0" }, true, undefined), { kind: "similar", sourceId: "z0" });
  assert.deepEqual(zoneCommitPlan({ mode: "cutout", sourceId: "gone" }, false, undefined), { kind: "none" });
  assert.deepEqual(zoneCommitPlan({ mode: "stitch", sourceId: "" }, false, undefined), { kind: "stitch" });
});

// ------------------------------------------------------------------------------------------------ UI-LEFT-2 PP10

test("PP10: the settings are sticky -- a new zone opens on the last OK'd zone's (GetDefaultZoneSettings), its layer, net and priority its own", () => {
  const sticky = { ...board, drawings: { zone_defaults: { clearance: 1_000_000, min_thickness: 300_000, fill_mode: "HatchPattern", name: "Pour", priority: 7, layers: ["F.Cu", "B.Cu"], is_rule_area: false } } } as unknown as BoardState;
  const z = newZoneInitial(sticky, { layer: "B.Cu", ruleArea: false, highlighted: [], selection: [], defaults });
  assert.deepEqual([z.settings.clearance, z.settings.min_thickness, z.settings.fill_mode, z.name, z.layers, z.settings.priority], [1_000_000, 300_000, "HatchPattern", "Pour", ["B.Cu"], 2]);
  // A rule area: no net, no name, and `ZONE_SETTINGS()`' keep-outs, the last zone OK'd being a pour (a pour here holds none of its own)
  const ra = newZoneInitial(sticky, { layer: "F.Cu", ruleArea: true, highlighted: [], selection: [], defaults });
  assert.deepEqual([ra.name, ra.net, ra.settings.keepout_tracks, ra.settings.keepout_copper_pour], ["", "", true, false]);
  // Zone Properties' OK: the settings, the layers and the name, no net, no outline
  const cmd = zoneDefaultsCmd({ ...defaults, clearance: 400_000 }, ["F.Cu", "In1.Cu"], "GND pour");
  assert.ok(cmd.op === "set_zone_defaults");
  if (cmd.op === "set_zone_defaults") {
    assert.deepEqual([cmd.zone.net, cmd.zone.layer, cmd.zone.outline, cmd.zone.layers, cmd.zone.name, cmd.zone.clearance], ["", "F.Cu", [], ["F.Cu", "In1.Cu"], "GND pour", 400_000]);
    assert.equal("id" in cmd.zone, false);
  }
  assert.deepEqual(stickySettings(undefined), {});
});

test("PP10: Properties (E) while drawing a cutout or similar zone opens the zone made from the source; OK's settings make the similar zone", () => {
  const src = { ...zone("z0", 4), name: "Old", layers: ["F.Cu", "B.Cu"], clearance: 750_000 } as never;
  const z = zoneFromExisting(src, true);
  assert.deepEqual([z.net, z.layers, z.name, z.locked, z.settings.clearance, z.settings.priority], ["GND", ["F.Cu", "B.Cu"], "Old", true, 750_000, 4]);
  const edited = { ...z, settings: { ...z.settings, clearance: 900_000 } };
  assert.deepEqual(zoneCommitPlan({ mode: "similar", sourceId: "z0" }, true, edited), { kind: "similar", sourceId: "z0", zone: edited });
  assert.deepEqual(zoneCommitPlan({ mode: "cutout", sourceId: "z0" }, true, edited), { kind: "cutout", sourceId: "z0" }, "a cutout takes only the outline");
});
