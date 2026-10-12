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

test("defaultDimensionPayload takes the text size and line thickness of the layer's class when the board has the table", () => {
  const defaults = { silk: { line_width_um: 120, text_width_um: 1200, text_height_um: 1300, text_thickness_um: 180, italic: false, upright: true }, copper: { line_width_um: 200, text_width_um: 1500, text_height_um: 1500, text_thickness_um: 300, italic: false, upright: true }, edge_cuts_line_width_um: 50, courtyard_line_width_um: 50, fab: { line_width_um: 90, text_width_um: 800, text_height_um: 800, text_thickness_um: 120, italic: false, upright: true }, others: { line_width_um: 110, text_width_um: 900, text_height_um: 900, text_thickness_um: 140, italic: false, upright: true } };
  const onSilk = defaultDimensionPayload("aligned", [0, 0], [1000, 0], "F.SilkS", SETTINGS, defaults);
  assert.deepEqual([onSilk.text_size_um, onSilk.stroke_width], [1300, 120]);
  const onFab = defaultDimensionPayload("aligned", [0, 0], [1000, 0], "F.Fab", SETTINGS, defaults);
  assert.deepEqual([onFab.text_size_um, onFab.stroke_width], [800, 90]);
  const without = defaultDimensionPayload("aligned", [0, 0], [1000, 0], "F.Fab", SETTINGS);
  assert.deepEqual([without.text_size_um, without.stroke_width], [SETTINGS.text_size_um, SETTINGS.stroke_width], "no table from the backend: the Dimensions page's own");
  assert.equal(onSilk.arrow_length, SETTINGS.arrow_length, "the arrows and extension lines are still the Dimensions page's");
});
