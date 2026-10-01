import { test } from "node:test";
import assert from "node:assert/strict";
import { cmdDimensionKindOf, defaultDimensionPayload, toCmdDimension } from "./dimensionConvert";
import type { Dimension, DimensionSettings } from "../api/types";

const SETTINGS: DimensionSettings = {
  units: "automatic",
  units_format: "no_suffix",
  precision: 4,
  suppress_trailing_zeros: true,
  text_position: "outside",
  keep_text_aligned: true,
  text_size_um: 1000,
  stroke_width: 200,
  arrow_length: 1270,
  extension_offset: 500,
  extension_height: 586,
};

function dim(overrides: Partial<Dimension> = {}): Dimension {
  return {
    id: "dim_1",
    layer: "Dwgs.User",
    kind: "aligned",
    height: 1000,
    horizontal: null,
    leader_length: null,
    start: [0, 0],
    end: [1000, 0],
    prefix: "",
    suffix: "",
    override_text: null,
    units: "mm",
    units_format: "no_suffix",
    precision: 2,
    suppress_trailing_zeros: true,
    text_position: "outside",
    keep_text_aligned: true,
    text_angle: 0,
    text_size_um: 1000,
    stroke_width: 150,
    arrow_length: 1000,
    extension_offset: 200,
    extension_height: 500,
    arrow_direction: "outward",
    lines: [],
    text_at: [500, -1000],
    computed_text_angle: 0,
    measured_value_um: 1000,
    text: "1",
    ...overrides,
  };
}

test("cmdDimensionKindOf reconstructs each kind's own fields", () => {
  assert.deepEqual(cmdDimensionKindOf(dim({ kind: "aligned", height: 1234 })), { kind: "aligned", height: 1234 });
  assert.deepEqual(cmdDimensionKindOf(dim({ kind: "orthogonal", height: 500, horizontal: false })), { kind: "orthogonal", height: 500, horizontal: false });
  assert.deepEqual(cmdDimensionKindOf(dim({ kind: "radial", leader_length: 700 })), { kind: "radial", leader_length: 700 });
  assert.deepEqual(cmdDimensionKindOf(dim({ kind: "leader" })), { kind: "leader" });
  assert.deepEqual(cmdDimensionKindOf(dim({ kind: "center" })), { kind: "center" });
});

test("toCmdDimension turns [x,y] pairs into PointXY objects and drops the computed geometry fields", () => {
  const cmd = toCmdDimension(dim({ start: [100, 200], end: [300, 400] }));
  assert.deepEqual(cmd.start, { x: 100, y: 200 });
  assert.deepEqual(cmd.end, { x: 300, y: 400 });
  assert.deepEqual(cmd.kind, { kind: "aligned", height: 1000 });
  assert.equal((cmd as unknown as { lines?: unknown }).lines, undefined, "computed geometry must not leak into the Cmd payload");
});

test("toCmdDimension keeps the id so the caller can still send edit_dimension with it", () => {
  const cmd = toCmdDimension(dim({ id: "dim_abc123" }));
  assert.equal(cmd.id, "dim_abc123");
});

test("defaultDimensionPayload pulls format/style fields from DimensionSettings", () => {
  const cmd = defaultDimensionPayload("aligned", [0, 0], [1000, 0], "Dwgs.User", SETTINGS);
  assert.equal(cmd.layer, "Dwgs.User");
  assert.equal(cmd.arrow_length, 1270);
  assert.equal(cmd.precision, 4);
  assert.equal(cmd.units, "automatic");
  assert.deepEqual(cmd.kind, { kind: "aligned", height: 5000 });
});

test("defaultDimensionPayload guesses orthogonal's orientation from the gesture's dominant axis", () => {
  const horizontalDrag = defaultDimensionPayload("orthogonal", [0, 0], [5000, 100], "Dwgs.User", SETTINGS);
  assert.deepEqual(horizontalDrag.kind, { kind: "orthogonal", height: 5000, horizontal: true });
  const verticalDrag = defaultDimensionPayload("orthogonal", [0, 0], [100, 5000], "Dwgs.User", SETTINGS);
  assert.deepEqual(verticalDrag.kind, { kind: "orthogonal", height: 5000, horizontal: false });
});

test("defaultDimensionPayload builds a bare kind object for leader/center (no extra fields)", () => {
  assert.deepEqual(defaultDimensionPayload("leader", [0, 0], [1000, 0], "Dwgs.User", SETTINGS).kind, { kind: "leader" });
  assert.deepEqual(defaultDimensionPayload("center", [0, 0], [1000, 0], "Dwgs.User", SETTINGS).kind, { kind: "center" });
});
