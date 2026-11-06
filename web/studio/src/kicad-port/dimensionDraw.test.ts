import { test } from "node:test";
import assert from "node:assert/strict";
import { boardDimensionStyle, crossbarOf, dimensionEditPoints, dimensionPointAt, moveDimensionPoint, textAfterCrossbarChange } from "./dimensionDraw";
import { fpDimensionClick, fpDimensionMotion, fpNewDimension } from "./fpDrawTools";
import type { BoardState, CmdDimension, Dimension, DimensionSettings } from "../api/types";

const MM = 1_000_000;

/** A display dimension as `GET /api/state` sends it, from a command and the geometry the server would add. */
function display(cmd: CmdDimension, extra: Partial<Dimension> = {}): Dimension {
  const k = cmd.kind;
  const cb = crossbarOf(cmd);
  return {
    id: "dim_a",
    layer: cmd.layer,
    kind: k.kind,
    height: k.kind === "aligned" || k.kind === "orthogonal" ? k.height : null,
    horizontal: k.kind === "orthogonal" ? k.horizontal : null,
    leader_length: k.kind === "radial" ? k.leader_length : null,
    start: [cmd.start.x, cmd.start.y],
    end: [cmd.end.x, cmd.end.y],
    prefix: cmd.prefix,
    suffix: cmd.suffix,
    override_text: cmd.override_text ?? null,
    units: cmd.units,
    units_format: cmd.units_format,
    precision: cmd.precision,
    suppress_trailing_zeros: cmd.suppress_trailing_zeros,
    text_position: cmd.text_position,
    keep_text_aligned: cmd.keep_text_aligned,
    text_angle: cmd.text_angle,
    text_size_nm: cmd.text_size_nm,
    stroke_width: cmd.stroke_width,
    arrow_length: cmd.arrow_length,
    extension_offset: cmd.extension_offset,
    extension_height: cmd.extension_height,
    arrow_direction: cmd.arrow_direction,
    lines: [],
    text_at: cmd.text_pos ? [cmd.text_pos.x, cmd.text_pos.y] : cb ? [(cb[0][0] + cb[1][0]) / 2, (cb[0][1] + cb[1][1]) / 2 - 1.15 * MM] : [cmd.end.x, cmd.end.y],
    computed_text_angle: 0,
    measured_value_nm: 0,
    text: "20",
    text_pos: cmd.text_pos ? [cmd.text_pos.x, cmd.text_pos.y] : null,
    crossbar: cb,
    knee: null,
    ...extra,
  };
}

const SETTINGS: DimensionSettings = {
  units: "mm",
  units_format: "bare_suffix",
  precision: 2,
  suppress_trailing_zeros: false,
  text_position: "inline",
  keep_text_aligned: false,
  text_size_nm: 1 * MM,
  stroke_width: 200_000,
  arrow_length: 2 * MM,
  extension_offset: 300_000,
  extension_height: 586_420,
};

test("the board's dimension tool takes Board Setup's Dimension Properties and the layer's Text & Graphics defaults (setMeasurementAttributes)", () => {
  const board = { drawings: { dimension_settings: SETTINGS } } as unknown as BoardState;
  const style = boardDimensionStyle(board);
  const d = fpNewDimension("aligned", "Cmts.User", [0, 0], style);
  assert.deepEqual([d.units, d.units_format, d.precision, d.suppress_trailing_zeros, d.text_position, d.keep_text_aligned, d.arrow_length, d.extension_offset], ["mm", "bare_suffix", 2, false, "inline", false, 2 * MM, 300_000]);
  // the leader keeps its constructor's text settings, the board's arrows
  const leader = fpNewDimension("leader", "Cmts.User", [5, 5], style);
  assert.deepEqual([leader.units, leader.units_format, leader.keep_text_aligned, leader.arrow_length, leader.text_pos], ["inch", "no_suffix", false, 2 * MM, { x: 5, y: 5 }]);
  // a board that says nothing has KiCad's defaults
  assert.equal(fpNewDimension("aligned", "F.SilkS", [0, 0], boardDimensionStyle(null)).units, "automatic");
});

test("an aligned dimension is three clicks: origin, end, height; the height follows the cursor across the measured line", () => {
  const style = boardDimensionStyle({ drawings: { dimension_settings: SETTINGS } } as unknown as BoardState);
  let r = fpDimensionClick(null, "aligned", "Cmts.User", [0, 0], false, style);
  assert.equal(r.draw?.step, "end");
  r = fpDimensionClick(r.draw, "aligned", "Cmts.User", [20 * MM, 0], false, style);
  assert.equal(r.draw?.step, "height", "the second click goes on to the height");
  const moved = fpDimensionMotion(r.draw!, [10 * MM, -3 * MM], false);
  assert.equal(moved.dim.kind.kind === "aligned" && moved.dim.kind.height, -3 * MM);
  r = fpDimensionClick(r.draw, "aligned", "Cmts.User", [10 * MM, -3 * MM], false, style);
  assert.equal(r.draw, null);
  assert.equal(r.commit?.kind.kind === "aligned" && r.commit.kind.height, -3 * MM);
});

test("an orthogonal dimension drawn right to left keeps its orientation inside the box and takes KiCad's un-normalised box tests outside it", () => {
  let r = fpDimensionClick(null, "orthogonal", "Cmts.User", [20 * MM, 10 * MM], false);
  r = fpDimensionClick(r.draw, "orthogonal", "Cmts.User", [0, 0], false);
  assert.equal(r.draw?.step, "height");
  // above the box, between the x of the points: `cursorPos.x > GetLeft() && cursorPos.x < GetRight()` is false for a box drawn leftwards (left 20, right 0),
  // and so is the y test, so the orientation comes from the cursor's offset from the centre: |dy| 15 > |dx| 0 -> horizontal
  const m = fpDimensionMotion(r.draw!, [10 * MM, -10 * MM], false);
  assert.ok(m.dim.kind.kind === "orthogonal" && m.dim.kind.horizontal);
  assert.equal(m.dim.kind.kind === "orthogonal" && m.dim.kind.height, -20 * MM, "the offset from the start across");
});

test("a radial dimension and a leader are done at the second click; a centre mark's end is held to 45 degrees", () => {
  let r = fpDimensionClick(null, "radial", "Cmts.User", [0, 0], false);
  r = fpDimensionClick(r.draw, "radial", "Cmts.User", [5 * MM, 0], false);
  assert.ok(r.commit && r.draw === null);
  assert.ok(r.commit!.text_pos, "the text past the knee");
  let c = fpDimensionClick(null, "center", "Cmts.User", [0, 0], false);
  c = fpDimensionClick(c.draw, "center", "Cmts.User", [5 * MM, 1 * MM], false);
  assert.deepEqual(c.commit?.end, { x: 5 * MM, y: 0 });
  // a click on the origin again is not a dimension
  let z = fpDimensionClick(null, "leader", "Cmts.User", [1, 1], false);
  z = fpDimensionClick(z.draw, "leader", "Cmts.User", [1, 1], false);
  assert.equal(z.commit, null);
  assert.equal(z.draw?.step, "end");
});

const aligned = (height: number): CmdDimension => ({ ...fpNewDimension("aligned", "Cmts.User", [0, 0]), start: { x: 0, y: 0 }, end: { x: 20 * MM, y: 0 }, kind: { kind: "aligned", height } });

test("the point editor's handles: the feature points, the text and the crossbar ends of an aligned dimension; a centre mark has two", () => {
  const d = display(aligned(-2 * MM));
  assert.deepEqual(dimensionEditPoints(d).map((p) => p.id), ["start", "end", "text", "crossbar_start", "crossbar_end"]);
  assert.deepEqual(d.crossbar, [[0, -2 * MM], [20 * MM, -2 * MM]]);
  assert.equal(dimensionPointAt(d, [20 * MM + 100, -2 * MM], 50_000), "crossbar_end");
  assert.equal(dimensionPointAt(d, [7 * MM, 7 * MM], 50_000), null);
  const centre = display({ ...fpNewDimension("center", "Cmts.User", [0, 0]), end: { x: MM, y: 0 } });
  assert.equal(dimensionEditPoints(centre).length, 2);
});

test("dragging an aligned dimension's crossbar end sets its height along the feature line; dragging its text places it by hand", () => {
  const d = display(aligned(-2 * MM));
  // off the line: projected onto it (`EC_LINE`), the side by the cross product
  const higher = moveDimensionPoint(d, "crossbar_end", [21 * MM, -5 * MM]);
  assert.equal(higher.kind.kind === "aligned" && higher.kind.height, -5 * MM);
  const below = moveDimensionPoint(d, "crossbar_start", [0, 3 * MM]);
  assert.equal(below.kind.kind === "aligned" && below.kind.height, 3 * MM);
  assert.equal(below.text_position, d.text_position, "the text mode is left alone");
  const text = moveDimensionPoint(d, "text", [4 * MM, -6 * MM]);
  assert.equal(text.text_position, "manual");
  assert.deepEqual(text.text_pos, { x: 4 * MM, y: -6 * MM });
});

test("a text placed by hand follows the crossbar (DIM_ALIGNED_TEXT_UPDATER): over it, it keeps its place along and across; past an end, its offset from that end", () => {
  // over the crossbar a quarter of the way along, 1 mm above it; the crossbar moves 3 mm up and doubles in length
  assert.deepEqual(textAfterCrossbarChange([5 * MM, -3 * MM], [[0, -2 * MM], [20 * MM, -2 * MM]], [[0, -5 * MM], [40 * MM, -5 * MM]]), [10 * MM, -6 * MM]);
  // past the end: 2 mm beyond it, along the line
  assert.deepEqual(textAfterCrossbarChange([22 * MM, -2 * MM], [[0, -2 * MM], [20 * MM, -2 * MM]], [[0, -5 * MM], [30 * MM, -5 * MM]]), [32 * MM, -5 * MM]);
  // the crossbar turned a quarter: the offset turns with it
  const turned = textAfterCrossbarChange([10 * MM, -3 * MM], [[0, 0], [20 * MM, 0]], [[0, 0], [0, 20 * MM]]);
  assert.deepEqual(turned, [3 * MM, 10 * MM], "3 mm to the left of the way the crossbar runs, as before");
  // dragging the end of a dimension whose text was placed by hand moves the text with the crossbar
  const manual = display({ ...aligned(-2 * MM), text_position: "manual", text_pos: { x: 5 * MM, y: -3 * MM } });
  const longer = moveDimensionPoint(manual, "end", [40 * MM, 0]);
  assert.deepEqual(longer.text_pos, { x: 10 * MM, y: -3 * MM });
});

test("an orthogonal dimension's crossbar end turns it when it leaves the box; a radial one's knee sets the leader length and moves the text; a leader's text keeps to 45 degrees", () => {
  const ortho = display({ ...fpNewDimension("orthogonal", "Cmts.User", [0, 0]), end: { x: 20 * MM, y: 10 * MM }, kind: { kind: "orthogonal", height: -3 * MM, horizontal: true } });
  const right = moveDimensionPoint(ortho, "crossbar_end", [25 * MM, 5 * MM]);
  assert.ok(right.kind.kind === "orthogonal" && !right.kind.horizontal, "right of the box: vertical");
  assert.equal(right.kind.kind === "orthogonal" && right.kind.height, 25 * MM);
  const inside = moveDimensionPoint(ortho, "crossbar_start", [5 * MM, 5 * MM]);
  assert.ok(inside.kind.kind === "orthogonal" && inside.kind.horizontal, "inside the box: as it was");

  const radialCmd: CmdDimension = { ...fpNewDimension("radial", "Cmts.User", [0, 0]), end: { x: 5 * MM, y: 0 }, kind: { kind: "radial", leader_length: 2 * MM }, text_pos: { x: 10 * MM, y: 0 } };
  const radial = display(radialCmd, { knee: [7 * MM, 0], text_at: [10 * MM, 0] });
  assert.deepEqual(dimensionEditPoints(radial).map((p) => p.id), ["start", "end", "text", "knee"]);
  const longer = moveDimensionPoint(radial, "knee", [9 * MM, 1 * MM]);
  assert.equal(longer.kind.kind === "radial" && longer.kind.leader_length, 4 * MM, "on the radius");
  assert.deepEqual(longer.text_pos, { x: 12 * MM, y: 0 }, "the text moves with the knee");

  const leaderCmd: CmdDimension = { ...fpNewDimension("leader", "Cmts.User", [0, 0]), end: { x: 5 * MM, y: -5 * MM }, text_pos: { x: 10 * MM, y: -5 * MM } };
  const leader = display(leaderCmd, { text_at: [10 * MM, -5 * MM] });
  assert.deepEqual(moveDimensionPoint(leader, "text", [9 * MM, -4 * MM]).text_pos, { x: 9 * MM, y: -5 * MM }, "held to the horizontal from the end");
  assert.deepEqual(moveDimensionPoint(leader, "end", [6 * MM, -6 * MM]).text_pos, { x: 11 * MM, y: -6 * MM }, "the text moves with the end");
});
