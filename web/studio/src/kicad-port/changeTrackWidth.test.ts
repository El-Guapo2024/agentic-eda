import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { changeTrackWidthCmds, currentTrackWidth, type SizeChoice } from "./changeTrackWidth";
import { DEFAULT_CUSTOM_TRACK_VIA } from "./routerVia";

/** A board with a Default class of 0.25 mm tracks and 0.6 / 0.3 mm vias, a `HS` class with its own uVia, two pre-defined widths and two via sizes. */
function board(widths = [300_000, 500_000], vias = [{ diameter: 800_000, drill: 400_000 }, { diameter: 1_000_000, drill: 500_000 }]): BoardState {
  return {
    name: "t",
    layers: ["F.Cu", "B.Cu"],
    parts: [],
    board_rules: {
      track_width: 250_000,
      clearance: 200_000,
      via_diameter: 600_000,
      via_drill: 300_000,
      net_classes: [{ name: "HS", nets: ["CLK*"], microvia_diameter: 250_000, microvia_drill: 120_000, priority: 0 }],
      default_class: { name: "Default", nets: [], track_width: 250_000, via_diameter: 600_000, via_drill: 300_000, microvia_diameter: 300_000, microvia_drill: 100_000, priority: 0 },
    },
    routing: {
      tracks: [{ id: "t1", net: "N", layer: "F.Cu", width: 200_000, pts: [[0, 0], [5000, 0]] }],
      vias: [
        { id: "v1", net: "N", x: 0, y: 0, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" },
        { id: "u1", net: "CLK0", x: 1000, y: 0, d: 300_000, drill: 100_000, from: "F.Cu", to: "In1.Cu", type: "micro" },
        { id: "u2", net: "N", x: 2000, y: 0, d: 300_000, drill: 100_000, from: "F.Cu", to: "In1.Cu", type: "micro" },
      ],
      zones: [],
      track_width_presets: widths,
      via_presets: vias,
    },
  } as unknown as BoardState;
}

const pick = (track: number, via: number, useCustomTrackVia = false): SizeChoice => ({ trackWidthIndex: track, viaSizeIndex: via, useCustomTrackVia, custom: DEFAULT_CUSTOM_TRACK_VIA });

test("GetCurrentTrackWidth: the board's list entry at the index, read now; index 0 or past the list the Default class's; custom while custom sizes are used", () => {
  assert.equal(currentTrackWidth(board(), 2, false, 200_000), 500_000);
  // Board Setup > Pre-defined Sizes changed entry 2 after it was picked: the new width is the one asked for (round 7 R7-5).
  assert.equal(currentTrackWidth(board([300_000, 400_000]), 2, false, 200_000), 400_000);
  assert.equal(currentTrackWidth(board(), 0, false, 200_000), 250_000, "the net class's entry");
  assert.equal(currentTrackWidth(board(), 7, false, 200_000), 250_000, "past the list");
  assert.equal(currentTrackWidth(board(), 2, true, 175_000), 175_000, "the custom width");
});

test("Change Track Width / Via Size: tracks get GetCurrentTrackWidth, vias GetCurrentViaSize / GetCurrentViaDrill, read from the lists now (round 7 R7-5)", () => {
  // Entry 1 of each list was picked, then Board Setup changed both rows.
  const b = board([400_000, 500_000], [{ diameter: 900_000, drill: 450_000 }]);
  assert.deepEqual(changeTrackWidthCmds(b, ["t1", "v1"], pick(1, 1)), [
    { op: "edit_tracks_and_vias", ids: ["t1", "v1"], track_width: { kind: "value", nm: 400_000 }, via_size: { kind: "value", diameter: 900_000, drill: 450_000 } },
  ]);
  // "Use net class values" (index 0): the Default class's, after an edit of the Default class.
  const edited = board();
  edited.board_rules!.default_class = { ...edited.board_rules!.default_class!, track_width: 270_000, via_diameter: 650_000, via_drill: 320_000 };
  assert.deepEqual(changeTrackWidthCmds(edited, ["t1", "v1"], pick(0, 0)), [
    { op: "edit_tracks_and_vias", ids: ["t1", "v1"], track_width: { kind: "value", nm: 270_000 }, via_size: { kind: "value", diameter: 650_000, drill: 320_000 } },
  ]);
  // Custom sizes in use.
  const custom: SizeChoice = { trackWidthIndex: 2, viaSizeIndex: 2, useCustomTrackVia: true, custom: { trackNm: 180_000, viaDiameterNm: 700_000, viaDrillNm: 350_000 } };
  assert.deepEqual(changeTrackWidthCmds(board(), ["t1"], custom), [
    { op: "edit_tracks_and_vias", ids: ["t1"], track_width: { kind: "value", nm: 180_000 }, via_size: { kind: "value", diameter: 700_000, drill: 350_000 } },
  ]);
  assert.deepEqual(changeTrackWidthCmds(board(), [], pick(1, 1)), []);
});

test("Change Track Width / Via Size: a microvia gets its effective net class's uVia size and drill (edit_tool.cpp:1413-1418)", () => {
  assert.deepEqual(changeTrackWidthCmds(board(), ["t1", "u1", "u2"], pick(1, 1)), [
    { op: "edit_tracks_and_vias", ids: ["t1"], track_width: { kind: "value", nm: 300_000 }, via_size: { kind: "value", diameter: 800_000, drill: 400_000 } },
    { op: "edit_tracks_and_vias", ids: ["u1"], via_size: { kind: "value", diameter: 250_000, drill: 120_000 } },
    { op: "edit_tracks_and_vias", ids: ["u2"], via_size: { kind: "value", diameter: 300_000, drill: 100_000 } },
  ]);
});
