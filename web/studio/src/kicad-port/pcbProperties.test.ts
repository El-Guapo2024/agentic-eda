import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Cmd, Dimension, Shape, Zone } from "../api/types";
import { PCB_PROPERTIES, pcbContext, pcbEdit, pcbFriendlyName, pcbGrid, pcbItemOf, pcbItemsOf, pcbRow, shapeWith } from "./pcbProperties";

// ---------------------------------------------------------------------------------------------------------------------------- fixture

const zone = (over: Partial<Zone> = {}): Zone => ({
  id: "zone_a",
  net: "GND",
  layer: "F.Cu",
  teardrop: false,
  outline: [[0, 0], [1000, 0], [1000, 1000]],
  clearance: 500,
  min_thickness: 250,
  thermal_gap: 500,
  thermal_spoke_width: 500,
  pad_connection: "Thermal",
  priority: 0,
  island_removal_mode: "Always",
  min_island_area: 10_000_000,
  fill_mode: "Polygons",
  hatch_thickness: 1000,
  hatch_gap: 1500,
  hatch_orientation_mdeg: 0,
  hatch_smoothing_level: 0,
  hatch_smoothing_value: 0.1,
  hatch_hole_min_area: 0.15,
  hatch_border_algorithm: 1,
  is_rule_area: false,
  keepout_tracks: false,
  keepout_vias: false,
  keepout_pads: false,
  keepout_copper_pour: false,
  keepout_footprints: false,
  ...over,
});

const dim = (over: Partial<Dimension> = {}): Dimension => ({
  id: "dim_a",
  layer: "Dwgs.User",
  kind: "aligned",
  height: 2000,
  horizontal: null,
  leader_length: null,
  start: [70_000, 50_000],
  end: [80_000, 50_000],
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
  extension_offset: 300,
  extension_height: 500,
  arrow_direction: "inward",
  lines: [],
  text_at: [75_000, 48_000],
  computed_text_angle: 0,
  measured_value_um: 10_000,
  text: "10.00",
  ...over,
});

const board = (): BoardState =>
  ({
    name: "t",
    dir: "/t",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100,
    parts: [
      {
        ref: "U1",
        value: "MCU",
        package: "QFN",
        mpn: "STM32",
        footprint: "Package_QFP:LQFP-48",
        block: "core",
        placed: true,
        size: [4000, 4000],
        at: [30_000, 20_000],
        rot: 90,
        side: "top",
        pads: [
          { num: "1", net: "GND", x: 29_000, y: 19_000, w: 600, h: 600, round: false, th: false },
          { num: "2", net: "VCC", x: 31_000, y: 19_000, w: 800, h: 800, round: true, th: true },
          { num: "3", net: "VCC", x: 31_000, y: 21_000, w: 800, h: 400, round: true, th: false },
        ],
      },
      { ref: "R1", value: "10k", package: "0603", mpn: null, footprint: "R_0603", block: null, placed: true, size: null, at: [60_000, 20_000], rot: 0, side: "bottom", pads: [] },
      { ref: "C9", value: null, package: null, mpn: null, footprint: null, block: null, placed: false, size: null },
    ],
    rules: [],
    routing: {
      tracks: [
        { id: "trk_a", net: "GND", layer: "F.Cu", width: 200, pts: [[10_000, 40_000], [20_000, 40_000], [20_000, 50_000]] },
        { id: "trk_b", net: "VCC", layer: "F.Cu", width: 300, pts: [[0, 0], [5_000, 0]] },
        { id: "trk_arc", net: "GND", layer: "B.Cu", width: 200, pts: [[10_000, 10_000], [20_000, 10_000]], arc_mid: [15_000, 5_000] },
      ],
      vias: [{ id: "via_a", net: "GND", x: 20_000, y: 50_000, d: 600, drill: 300, from: "F.Cu", to: "B.Cu" }],
      zones: [zone(), zone({ id: "zone_ra", net: "", is_rule_area: true, keepout_vias: true, name: "keepout" })],
      track_width_presets: [],
      via_presets: [],
      teardrop_settings: {} as never,
    },
    drawings: {
      shapes: [
        { id: "shp_seg", kind: "segment", layer: "F.SilkS", stroke_width: 150, filled: false, start: [0, 80_000], end: [10_000, 80_000] },
        { id: "shp_rect", kind: "rect", layer: "F.Fab", stroke_width: 100, filled: false, start: [0, 90_000], end: [10_000, 95_000] },
        { id: "shp_arc", kind: "arc", layer: "F.SilkS", stroke_width: 150, filled: false, start: [40_000, 80_000], mid: [45_000, 77_000], end: [50_000, 80_000] },
        { id: "shp_circle", kind: "circle", layer: "F.SilkS", stroke_width: 150, filled: true, center: [5_000, 5_000], end: [8_000, 9_000] },
        { id: "shp_poly", kind: "polygon", layer: "Edge.Cuts", stroke_width: 100, filled: false, pts: [[0, 0], [10_000, 0], [10_000, 10_000]] },
        { id: "shp_bez", kind: "bezier", layer: "F.SilkS", stroke_width: 150, filled: false, start: [0, 0], c1: [1000, 1000], c2: [2000, 1000], end: [3000, 0] },
      ],
      texts: [{ id: "txt_a", content: "hello", x: 70_000, y: 80_000, angle: 90_000, layer: "F.SilkS", size: 1000, stroke_width: 150, justify: "center", mirror: false }],
      groups: [{ id: "grp_a", name: "block", member_ids: ["U1", "trk_a"] }],
      dimensions: [
        dim(),
        dim({ id: "dim_rad", kind: "radial", height: null, leader_length: 3000 }),
        dim({ id: "dim_lead", kind: "leader", height: null, override_text: "NOTE" }),
        dim({ id: "dim_ortho", kind: "orthogonal", horizontal: true }),
        dim({ id: "dim_ctr", kind: "center", height: null }),
      ],
      dimension_settings: {} as never,
    },
    checks: [],
    locked: ["trk_b"],
    activity: [],
    job: "idle",
  }) as unknown as BoardState;

const names = (b: BoardState, ids: string[]): Array<[string, string[]]> => pcbGrid(b, ids, "mm").groups.map((g) => [g.caption, g.rows.map((r) => r.name)]);
const rowOf = (b: BoardState, ids: string[], name: string) => pcbGrid(b, ids, "mm").groups.flatMap((g) => g.rows).find((r) => r.name === name);
const edit = (b: BoardState, ids: string[], name: string, value: string | number | boolean, units: "mm" | "mil" | "in" = "mm"): Cmd[] => {
  const plan = pcbEdit(b, ids, name, value, units);
  assert.equal(plan.ok, true, plan.ok ? "" : plan.error);
  return plan.ok ? plan.cmds : [];
};

// ---------------------------------------------------------------------------------------------------------------- item resolution

test("a selection id names the kind of item it is: parts, pads, tracks and arcs, vias, zones, shapes, texts, the five dimensions and groups", () => {
  const b = board();
  const kinds = Object.fromEntries(["U1", "U1.2", "trk_a", "trk_arc", "via_a", "zone_a", "shp_seg", "txt_a", "dim_a", "dim_ortho", "dim_rad", "dim_lead", "dim_ctr", "grp_a"].map((id) => [id, pcbItemOf(b, id)?.type]));
  assert.deepEqual(kinds, {
    U1: "FOOTPRINT",
    "U1.2": "PAD",
    trk_a: "PCB_TRACK",
    trk_arc: "PCB_ARC",
    via_a: "PCB_VIA",
    zone_a: "ZONE",
    shp_seg: "PCB_SHAPE",
    txt_a: "PCB_TEXT",
    dim_a: "PCB_DIM_ALIGNED",
    dim_ortho: "PCB_DIM_ORTHOGONAL",
    dim_rad: "PCB_DIM_RADIAL",
    dim_lead: "PCB_DIM_LEADER",
    dim_ctr: "PCB_DIM_CENTER",
    grp_a: "PCB_GROUP",
  });
  assert.equal(pcbItemOf(b, "C9"), null, "an unplaced part is not on the board");
  assert.deepEqual(pcbItemsOf(b, ["trk_a", "nope", "via_a"]).map((i) => i.id), ["trk_a", "via_a"], "an id the board lacks is skipped; order is kept");
});

test("the caption of a single item is its friendly name", () => {
  const b = board();
  const caption = (id: string) => pcbGrid(b, [id], "mm").caption;
  assert.equal(caption("U1"), "Footprint");
  assert.equal(caption("U1.1"), "Pad");
  assert.equal(caption("trk_a"), "Track");
  assert.equal(caption("trk_arc"), "Track (arc)");
  assert.equal(caption("via_a"), "Via");
  assert.equal(caption("zone_a"), "Copper Zone");
  assert.equal(caption("zone_ra"), "Rule Area");
  assert.equal(caption("shp_seg"), "Graphic");
  assert.equal(caption("txt_a"), "Text");
  assert.equal(caption("dim_a"), "Dimension");
  assert.equal(caption("dim_lead"), "Leader");
  assert.equal(caption("grp_a"), "Group");
  assert.equal(pcbGrid(b, ["U1", "trk_a", "via_a"], "mm").caption, "3 objects selected");
  assert.equal(pcbGrid(b, [], "mm").caption, "No objects selected");
  assert.equal(pcbFriendlyName(pcbItemOf(b, "zone_a")!), "Copper Zone");
});

// ------------------------------------------------------------------------------------------------------------------ property lists

test("a footprint: position, side and orientation, then its fields, library link, and where the intent put it", () => {
  assert.deepEqual(names(board(), ["U1"]), [
    ["Basic Properties", ["Position X", "Position Y", "Locked", "Layer", "Orientation"]],
    ["Fields", ["Reference", "Value", "MPN"]],
    ["Footprint Properties", ["Library Link"]],
    ["Placement", ["Block", "Nets"]],
  ]);
  // No MPN and no block: the rows that need one are not there.
  assert.deepEqual(names(board(), ["R1"]), [
    ["Basic Properties", ["Position X", "Position Y", "Locked", "Layer", "Orientation"]],
    ["Fields", ["Reference", "Value"]],
    ["Footprint Properties", ["Library Link"]],
    ["Placement", ["Nets"]],
  ]);
});

test("a pad: where it is, and what it is; its number and net are read-only, its type, shape, size and hole are edited", () => {
  const b = board();
  assert.deepEqual(names(b, ["U1.1"]), [
    ["Basic Properties", ["Position X", "Position Y", "Net"]],
    ["Pad Properties", ["Pad Type", "Pad Shape", "Pad Number", "Size X", "Size Y", "Hole Shape", "Hole Size X"]],
  ]);
  const writable = pcbGrid(b, ["U1.1"], "mm").groups.flatMap((g) => g.rows).filter((r) => r.writable).map((r) => r.name);
  assert.deepEqual(writable, ["Position X", "Position Y", "Pad Type", "Pad Shape", "Size X", "Size Y"], "an SMD pad has no hole to edit; its net and number have no verb");
  const th = pcbGrid(b, ["U1.2"], "mm").groups.flatMap((g) => g.rows).filter((r) => r.writable).map((r) => r.name);
  assert.deepEqual(th, ["Position X", "Position Y", "Pad Type", "Pad Shape", "Size X", "Hole Shape", "Hole Size X"], "a through-hole pad can edit its hole");
  // A pad that does not say its kind and shape (an older backend) is read from its flags.
  assert.equal(rowOf(b, ["U1.2"], "Pad Type")!.value, "through_hole");
  assert.equal(rowOf(b, ["U1.2"], "Pad Shape")!.value, "circle");
  assert.equal(rowOf(b, ["U1.3"], "Pad Shape")!.value, "oval");
  assert.equal(rowOf(b, ["U1.1"], "Pad Shape")!.value, "rect");
  assert.equal(rowOf(b, ["U1.2"], "Size Y"), undefined, "a circle has no height");
});

test("a track and an arc: lock, layer, net, then width and the two end points", () => {
  const b = board();
  const track: Array<[string, string[]]> = [["Basic Properties", ["Locked", "Layer", "Net", "Width", "Start X", "Start Y", "End X", "End Y"]]];
  assert.deepEqual(names(b, ["trk_a"]), track);
  assert.deepEqual(names(b, ["trk_arc"]), track);
});

test("a via: position, lock, net, and its Via Properties group", () => {
  assert.deepEqual(names(board(), ["via_a"]), [
    ["Basic Properties", ["Position X", "Position Y", "Locked", "Net"]],
    ["Via Properties", ["Diameter", "Hole", "Layer Top", "Layer Bottom", "Via Type"]],
  ]);
});

test("a copper zone and a rule area: what each has, and none of a zone's position or layer", () => {
  const b = board();
  assert.deepEqual(names(b, ["zone_a"]), [
    ["Basic Properties", ["Locked", "Net", "Priority", "Name"]],
    ["Fill Style", ["Fill Mode", "Hatch Orientation", "Hatch Width", "Hatch Gap", "Hatch Minimum Hole Ratio", "Smoothing Effort", "Smoothing Amount", "Remove Islands", "Minimum Island Area"]],
    ["Electrical", ["Clearance", "Minimum Width", "Pad Connections", "Thermal Relief Gap", "Thermal Relief Spoke Width"]],
  ]);
  assert.deepEqual(names(b, ["zone_ra"]), [
    ["Basic Properties", ["Locked", "Name"]],
    ["Keepout", ["Keep Out Tracks", "Keep Out Vias", "Keep Out Pads", "Keep Out Zone Fills", "Keep Out Footprints"]],
  ]);
});

test("text: position, layer, lock, orientation, then its Text Properties", () => {
  assert.deepEqual(names(board(), ["txt_a"]), [
    ["Basic Properties", ["Position X", "Position Y", "Layer", "Locked", "Orientation"]],
    ["Text Properties", ["Text", "Thickness", "Mirrored", "Width", "Height", "Horizontal Justification"]],
  ]);
});

test("a shape shows the geometry its kind has", () => {
  const b = board();
  const shape = (id: string) => names(b, [id]).find(([g]) => g === "Shape Properties")![1];
  assert.deepEqual(shape("shp_seg"), ["Shape", "Start X", "Start Y", "End X", "End Y", "Line Width"]);
  assert.deepEqual(shape("shp_rect"), ["Shape", "Start X", "Start Y", "End X", "End Y", "Width", "Height", "Line Width", "Fill"]);
  assert.deepEqual(shape("shp_arc"), ["Shape", "Start X", "Start Y", "End X", "End Y", "Line Width", "Angle"]);
  assert.deepEqual(shape("shp_circle"), ["Shape", "Center X", "Center Y", "Radius", "Line Width", "Fill"]);
  assert.deepEqual(shape("shp_poly"), ["Shape", "Line Width", "Fill"]);
  assert.deepEqual(shape("shp_bez"), ["Shape", "Start X", "Start Y", "End X", "End Y", "Line Width"], "a Bezier is not filled");
  // Position is the polygon's alone; every shape has a layer and a lock; none has a net.
  assert.deepEqual(names(b, ["shp_poly"])[0], ["Basic Properties", ["Position X", "Position Y", "Locked", "Layer"]]);
  assert.deepEqual(names(b, ["shp_seg"])[0], ["Basic Properties", ["Locked", "Layer"]]);
});

test("dimensions: the kinds share the text and layer rows and each adds its own", () => {
  const b = board();
  const rows = (id: string) => Object.fromEntries(names(b, [id]));
  assert.deepEqual(rows("dim_a")["Dimension Properties"], ["Prefix", "Suffix", "Override Text", "Units", "Units Format", "Precision", "Suppress Trailing Zeroes", "Arrow Direction", "Crossbar Height", "Extension Line Overshoot"]);
  assert.deepEqual(rows("dim_ortho")["Dimension Properties"], rows("dim_a")["Dimension Properties"], "an orthogonal dimension is an aligned one with a locked axis");
  assert.deepEqual(rows("dim_rad")["Dimension Properties"], ["Prefix", "Suffix", "Override Text", "Units", "Units Format", "Precision", "Suppress Trailing Zeroes", "Leader Length"]);
  assert.deepEqual(rows("dim_lead")["Dimension Properties"], ["Text"], "a leader has its text and none of the measuring");
  assert.deepEqual(rows("dim_ctr")["Dimension Properties"], ["Prefix", "Suffix", "Override Text", "Units", "Units Format", "Precision", "Suppress Trailing Zeroes"], "a centre mark has the measuring rows and none of the geometry");
  assert.deepEqual(rows("dim_a")["Basic Properties"], ["Position X", "Position Y", "Layer", "Locked"]);
  assert.deepEqual(rows("dim_a")["Text Properties"], ["Thickness", "Width", "Height", "Keep Aligned with Dimension", "Orientation"]);
});

test("a group has a name and a lock, and no position or layer", () => {
  assert.deepEqual(names(board(), ["grp_a"]), [
    ["Basic Properties", ["Locked"]],
    ["Group Properties", ["Name"]],
  ]);
});

test("the registry never lists a property twice for one class, and a Layer is the only name that comes from more than one place", () => {
  for (const type of ["FOOTPRINT", "PAD", "PCB_TRACK", "PCB_ARC", "PCB_VIA", "ZONE", "PCB_TEXT", "PCB_SHAPE", "PCB_DIM_ALIGNED", "PCB_DIM_ORTHOGONAL", "PCB_DIM_RADIAL", "PCB_DIM_LEADER", "PCB_DIM_CENTER", "PCB_GROUP"]) {
    const seen = new Map<string, string>();
    for (const p of PCB_PROPERTIES.getProperties(type)) {
      const owner = seen.get(p.name);
      assert.ok(owner === undefined || p.name === "Text" || p.name === "Orientation", `${type}: ${p.name} from ${owner} and ${p.owner}`);
      seen.set(p.name, p.owner);
    }
  }
});

// -------------------------------------------------------------------------------------------------------------------------- values

test("a footprint's orientation is KiCad's (counter-clockwise), the model's rot runs clockwise", () => {
  const b = board();
  assert.equal(rowOf(b, ["U1"], "Orientation")!.value, 270, "rot 90 clockwise is 270 counter-clockwise");
  assert.equal(rowOf(b, ["R1"], "Orientation")!.value, 0);
  assert.equal(rowOf(b, ["U1"], "Layer")!.value, "F.Cu");
  assert.equal(rowOf(b, ["R1"], "Layer")!.value, "B.Cu");
  assert.deepEqual(rowOf(b, ["U1"], "Layer")!.choices, [{ label: "F.Cu", value: "F.Cu" }, { label: "B.Cu", value: "B.Cu" }]);
});

test("a track shows its end points; a via its type; a locked item its lock", () => {
  const b = board();
  assert.deepEqual([rowOf(b, ["trk_a"], "Start X")!.value, rowOf(b, ["trk_a"], "Start Y")!.value, rowOf(b, ["trk_a"], "End X")!.value, rowOf(b, ["trk_a"], "End Y")!.value], [10_000, 40_000, 20_000, 50_000]);
  assert.equal(rowOf(b, ["via_a"], "Via Type")!.value, "Through");
  assert.equal(rowOf(b, ["trk_b"], "Locked")!.value, true);
  assert.equal(rowOf(b, ["trk_a"], "Locked")!.value, false);
});

test("a circle shows its radius from the centre to the point on it, a rectangle its sides, an arc the angle it sweeps", () => {
  const b = board();
  assert.equal(rowOf(b, ["shp_circle"], "Radius")!.value, 5000);
  assert.equal(rowOf(b, ["shp_circle"], "Center X")!.value, 5000);
  assert.equal(rowOf(b, ["shp_rect"], "Width")!.value, 10_000);
  assert.equal(rowOf(b, ["shp_rect"], "Height")!.value, 5000);
  const angle = rowOf(b, ["shp_arc"], "Angle")!;
  assert.ok(typeof angle.value === "number" && angle.value > 0 && angle.value < 180);
  assert.equal(angle.writable, false);
});

test("several items: the value they share, or none when they differ", () => {
  const b = board();
  assert.equal(rowOf(b, ["trk_a", "trk_arc"], "Net")!.value, "GND");
  assert.equal(rowOf(b, ["trk_a", "trk_b"], "Net")!.value, null, "GND and VCC");
  assert.equal(rowOf(b, ["trk_a", "trk_b"], "Width")!.value, null);
  assert.equal(rowOf(b, ["trk_a", "trk_b"], "Locked")!.value, null, "one is locked");
  assert.equal(rowOf(b, ["trk_a", "trk_b"], "Layer")!.value, "F.Cu");
});

test("a mixed selection keeps the rows every class has, in the order of the first one", () => {
  const b = board();
  // A footprint's layers are the front and the back, and so are a track's on this board: the row is shared. A via has no layer; a text's are every layer.
  assert.deepEqual(names(b, ["U1", "trk_a"]), [["Basic Properties", ["Locked", "Layer"]]]);
  assert.deepEqual(names(b, ["trk_a", "U1"]), [["Basic Properties", ["Locked", "Layer"]]]);
  assert.deepEqual(names(b, ["U1", "via_a"]), [["Basic Properties", ["Position X", "Position Y", "Locked"]]]);
  assert.deepEqual(names(b, ["trk_a", "txt_a"]), [["Basic Properties", ["Locked", "Width"]]], "the layers differ, so no Layer; Width is a track's and a text's alike (same name)");
});

// ---------------------------------------------------------------------------------------------------------------------------- edits

test("a footprint: position moves it by the difference, orientation turns it about itself, layer flips it", () => {
  const b = board();
  assert.deepEqual(edit(b, ["U1"], "Position X", 32_000), [{ op: "move_items", ids: ["U1"], dx: 2_000, dy: 0 }]);
  assert.deepEqual(edit(b, ["U1"], "Position Y", 19_000), [{ op: "move_items", ids: ["U1"], dx: 0, dy: -1_000 }]);
  assert.deepEqual(edit(b, ["U1"], "Position X", 30_000), [], "already there: nothing to send");
  // 270 is where it is (rot 90 clockwise); 0 is a quarter turn counter-clockwise from 270... clockwise from rot 90 to rot 0 is -90.
  assert.deepEqual(edit(b, ["U1"], "Orientation", 0), [{ op: "rotate_items", ids: ["U1"], pivot: { x: 30_000, y: 20_000 }, angle_millideg: -90_000 }]);
  assert.deepEqual(edit(b, ["U1"], "Orientation", 270), []);
  assert.deepEqual(edit(b, ["U1"], "Orientation", 90), [{ op: "rotate_items", ids: ["U1"], pivot: { x: 30_000, y: 20_000 }, angle_millideg: -180_000 }]);
  assert.deepEqual(edit(b, ["U1"], "Layer", "B.Cu"), [{ op: "flip_items", ids: ["U1"], pivot: { x: 30_000, y: 20_000 }, direction: "left_right" }]);
  assert.deepEqual(edit(b, ["U1"], "Layer", "F.Cu"), []);
  assert.deepEqual(edit(b, ["U1"], "Locked", true), [{ op: "set_locked", ids: ["U1"], locked: true }]);
});

test("a footprint's reference, value and library link are read-only", () => {
  const b = board();
  for (const name of ["Reference", "Value", "Library Link", "Nets"]) assert.equal(rowOf(b, ["U1"], name)!.writable, false, name);
  assert.deepEqual(edit(b, ["U1"], "Value", "other"), [], "an edit of a read-only row sends nothing");
});

test("a pad's position moves its footprint", () => {
  const b = board();
  assert.deepEqual(edit(b, ["U1.1"], "Position X", 29_500), [{ op: "move_items", ids: ["U1"], dx: 500, dy: 0 }]);
  assert.deepEqual(edit(b, ["U1.1"], "Net", "VCC"), [], "a pad's net is not editable");
});

test("a track: width, layer, net and the end points each go through their verb", () => {
  const b = board();
  assert.deepEqual(edit(b, ["trk_a"], "Width", 250), [{ op: "set_track_width", id: "trk_a", width: 250 }]);
  assert.deepEqual(edit(b, ["trk_a"], "Width", 200), []);
  assert.deepEqual(edit(b, ["trk_a"], "Layer", "B.Cu"), [{ op: "edit_tracks_and_vias", ids: ["trk_a"], layer: "B.Cu" }]);
  assert.deepEqual(edit(b, ["trk_a"], "Net", "VCC"), [{ op: "set_item_net", ids: ["trk_a"], net: "VCC" }]);
  assert.deepEqual(edit(b, ["trk_a"], "Start X", 11_000), [{ op: "edit_track", id: "trk_a", start: { x: 11_000, y: 40_000 } }]);
  assert.deepEqual(edit(b, ["trk_a"], "End Y", 52_000), [{ op: "edit_track", id: "trk_a", end: { x: 20_000, y: 52_000 } }]);
  assert.deepEqual(edit(b, ["trk_arc"], "End X", 24_000), [{ op: "edit_track", id: "trk_arc", end: { x: 24_000, y: 10_000 } }]);
});

test("a width of zero or less is refused, and so is taking a track off its net", () => {
  const b = board();
  const w = pcbEdit(b, ["trk_a"], "Width", 0, "mm");
  assert.equal(w.ok, false);
  assert.match(!w.ok ? w.error : "", /^Width: Value must be greater than or equal to 0\.001 mm$/);
  const n = pcbEdit(b, ["trk_a"], "Net", "", "mm");
  assert.equal(n.ok, false);
  assert.match(!n.ok ? n.error : "", /needs a net/);
});

test("a via: diameter and hole keep each other in order", () => {
  const b = board();
  assert.deepEqual(edit(b, ["via_a"], "Diameter", 700), [{ op: "edit_via", id: "via_a", diameter: 700, drill: 300 }]);
  assert.deepEqual(edit(b, ["via_a"], "Hole", 350), [{ op: "edit_via", id: "via_a", diameter: 600, drill: 350 }]);
  const big = pcbEdit(b, ["via_a"], "Hole", 700, "mm");
  assert.equal(big.ok, false);
  assert.match(!big.ok ? big.error : "", /^Hole: Via hole size must be smaller than via diameter$/);
  const small = pcbEdit(b, ["via_a"], "Diameter", 300, "mm");
  assert.equal(small.ok, false, "the diameter may not shrink to the hole");
  assert.deepEqual(edit(b, ["via_a"], "Net", "VCC"), [{ op: "set_item_net", ids: ["via_a"], net: "VCC" }]);
  assert.equal(rowOf(b, ["via_a"], "Layer Top")!.writable, false);
});

test("a zone: one edit_zone with every setting and the one that changed", () => {
  const b = board();
  const [cmd] = edit(b, ["zone_a"], "Priority", 3);
  const c = cmd as unknown as Record<string, unknown>;
  assert.equal(c.op, "edit_zone");
  assert.deepEqual(
    { id: c.id, priority: c.priority, net: c.net, layer: c.layer, clearance: c.clearance, min_thickness: c.min_thickness, thermal_gap: c.thermal_gap, fill_mode: c.fill_mode, is_rule_area: c.is_rule_area, hatch_gap: c.hatch_gap },
    { id: "zone_a", priority: 3, net: "GND", layer: "F.Cu", clearance: 500, min_thickness: 250, thermal_gap: 500, fill_mode: "Polygons", is_rule_area: false, hatch_gap: 1500 },
    "the zone's other settings are sent as they are: the verb replaces them all"
  );
  assert.deepEqual(edit(b, ["zone_a"], "Fill Mode", "HatchPattern").map((c) => (c as { fill_mode?: string }).fill_mode), ["HatchPattern"]);
  assert.deepEqual(edit(b, ["zone_a"], "Remove Islands", "Area").map((c) => (c as { island_removal_mode?: string }).island_removal_mode), ["Area"]);
  assert.deepEqual(edit(b, ["zone_a"], "Hatch Orientation", 45).length, 0, "hatch settings are read-only until the fill is hatched");
  assert.deepEqual(edit(b, ["zone_a"], "Net", "VCC"), [{ op: "set_item_net", ids: ["zone_a"], net: "VCC" }]);
  assert.deepEqual(edit(b, ["zone_a"], "Name", "pour"), [{ op: "set_zone_name", id: "zone_a", name: "pour" }]);
  assert.deepEqual(edit(b, ["zone_ra"], "Keep Out Tracks", true).map((c) => (c as { keepout_tracks?: boolean }).keepout_tracks), [true]);
  assert.deepEqual(edit(b, ["zone_ra"], "Keep Out Vias", true), [], "already on");
});

test("a hatched zone's hatch rows can be edited, and the validators of the zone dialog hold", () => {
  const b = board();
  b.routing!.zones[0] = zone({ fill_mode: "HatchPattern" });
  assert.equal(rowOf(b, ["zone_a"], "Hatch Width")!.writable, true);
  assert.equal(edit(b, ["zone_a"], "Hatch Orientation", 45).length, 1);
  const narrow = pcbEdit(b, ["zone_a"], "Hatch Width", 100, "mm");
  assert.equal(narrow.ok, false);
  assert.match(!narrow.ok ? narrow.error : "", /Cannot be less than zone minimum width/);
  const spoke = pcbEdit(b, ["zone_a"], "Thermal Relief Spoke Width", 100, "mm");
  assert.equal(spoke.ok, false);
  const clear = pcbEdit(b, ["zone_a"], "Clearance", -5, "mm");
  assert.equal(clear.ok, false);
  const thin = pcbEdit(b, ["zone_a"], "Minimum Width", 10, "mm");
  assert.equal(thin.ok, false);
});

test("a text: every edit is an edit_text with the rest of the text as it was", () => {
  const b = board();
  const base = { op: "edit_text", id: "txt_a", content: "hello", angle: 90_000, layer: "F.SilkS", size_um: 1000, stroke_width: 150, justify: "center", mirror: false };
  assert.deepEqual(edit(b, ["txt_a"], "Text", "bye"), [{ ...base, content: "bye" }]);
  assert.deepEqual(edit(b, ["txt_a"], "Orientation", 180), [{ ...base, angle: 180_000 }]);
  assert.deepEqual(edit(b, ["txt_a"], "Orientation", -90), [{ ...base, angle: 270_000 }], "an angle is kept in 0 to 360");
  assert.deepEqual(edit(b, ["txt_a"], "Layer", "F.Fab"), [{ ...base, layer: "F.Fab" }]);
  assert.deepEqual(edit(b, ["txt_a"], "Thickness", 200), [{ ...base, stroke_width: 200 }]);
  assert.deepEqual(edit(b, ["txt_a"], "Height", 1500), [{ ...base, size_um: 1500 }]);
  assert.deepEqual(edit(b, ["txt_a"], "Width", 1500), [{ ...base, size_um: 1500 }], "the text is square: width and height are one size");
  assert.deepEqual(edit(b, ["txt_a"], "Mirrored", true), [{ ...base, mirror: true }]);
  assert.deepEqual(edit(b, ["txt_a"], "Horizontal Justification", "left"), [{ ...base, justify: "left" }]);
  assert.deepEqual(edit(b, ["txt_a"], "Position X", 71_000), [{ op: "move_items", ids: ["txt_a"], dx: 1_000, dy: 0 }]);
});

test("a shape's geometry is set the way the EDA_SHAPE setters do", () => {
  const seg = board().drawings!.shapes[0] as Extract<Shape, { kind: "segment" }>;
  assert.deepEqual(shapeWith(seg, "Start X", 1000), { ...seg, start: [1000, 80_000] });
  assert.deepEqual(shapeWith(seg, "End Y", 70_000), { ...seg, end: [10_000, 70_000] });
  assert.equal(shapeWith(seg, "Start X", 0), null, "unchanged");
  assert.equal(shapeWith(seg, "Radius", 5), null, "a segment has no radius");

  const rect = board().drawings!.shapes[1] as Extract<Shape, { kind: "rect" }>;
  assert.deepEqual(shapeWith(rect, "Width", 12_000), { ...rect, end: [12_000, 95_000] });
  assert.deepEqual(shapeWith(rect, "Height", 2_000), { ...rect, end: [10_000, 92_000] });
  const flipped: Shape = { ...rect, start: [10_000, 95_000], end: [0, 90_000] };
  assert.deepEqual(shapeWith(flipped, "Width", 4_000), { ...flipped, end: [6_000, 90_000] }, "the corners keep their order");

  const circle = board().drawings!.shapes[3] as Extract<Shape, { kind: "circle" }>;
  assert.deepEqual(shapeWith(circle, "Center X", 6_000), { ...circle, center: [6_000, 5_000], end: [9_000, 9_000] }, "the point on the circle goes with the centre");
  assert.deepEqual(shapeWith(circle, "Radius", 2_000), { ...circle, end: [7_000, 5_000] }, "SetRadius puts the point to the right of the centre");
});

test("shape rows send a replace_shape for the geometry and an edit_shape for line, layer and fill", () => {
  const b = board();
  const [seg] = edit(b, ["shp_seg"], "End X", 12_000);
  assert.equal(seg!.op, "replace_shape");
  assert.deepEqual(seg, { op: "replace_shape", id: "shp_seg", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 150, filled: false, start: { x: 0, y: 80_000 }, end: { x: 12_000, y: 80_000 } } });
  assert.deepEqual(edit(b, ["shp_seg"], "Line Width", 300), [{ op: "edit_shape", id: "shp_seg", layer: "F.SilkS", stroke_width: 300, filled: false }]);
  assert.deepEqual(edit(b, ["shp_seg"], "Layer", "F.Fab"), [{ op: "edit_shape", id: "shp_seg", layer: "F.Fab", stroke_width: 150, filled: false }]);
  assert.deepEqual(edit(b, ["shp_rect"], "Fill", 1), [{ op: "edit_shape", id: "shp_rect", layer: "F.Fab", stroke_width: 100, filled: true }]);
  assert.deepEqual(edit(b, ["shp_poly"], "Position X", 1_000), [{ op: "move_items", ids: ["shp_poly"], dx: 1_000, dy: 0 }]);
  assert.equal(edit(b, ["shp_rect"], "Width", 10_000).length, 0, "the width it already has");
  assert.equal(pcbEdit(b, ["shp_circle"], "Radius", 0, "mm").ok, false);
});

test("a dimension: an edit_dimension that keeps everything else, the label's pen included", () => {
  const b = board();
  b.drawings!.dimensions[0] = dim({ text_thickness_um: 180 });
  const [cmd] = edit(b, ["dim_a"], "Prefix", "L=");
  assert.equal(cmd!.op, "edit_dimension");
  const d = (cmd as unknown as { dimension: Record<string, unknown> }).dimension;
  assert.equal(d.prefix, "L=");
  assert.equal(d.text_thickness_um, 180, "the pen survives an edit that does not mention it");
  assert.deepEqual(d.kind, { kind: "aligned", height: 2000 });
  assert.deepEqual(d.start, { x: 70_000, y: 50_000 });
  const [h] = edit(b, ["dim_a"], "Crossbar Height", 3000);
  assert.deepEqual((h as unknown as { dimension: { kind: unknown } }).dimension.kind, { kind: "aligned", height: 3000 });
  const [o] = edit(b, ["dim_ortho"], "Crossbar Height", 1000);
  assert.deepEqual((o as unknown as { dimension: { kind: unknown } }).dimension.kind, { kind: "orthogonal", height: 1000, horizontal: true });
  const [r] = edit(b, ["dim_rad"], "Leader Length", 4000);
  assert.deepEqual((r as unknown as { dimension: { kind: unknown } }).dimension.kind, { kind: "radial", leader_length: 4000 });
  const [p] = edit(b, ["dim_a"], "Position X", 71_000);
  assert.deepEqual((p as unknown as { dimension: { start: unknown; end: unknown } }).dimension.start, { x: 71_000, y: 50_000 }, "the first feature point alone, as SetPosition does");
  assert.deepEqual((p as unknown as { dimension: { start: unknown; end: unknown } }).dimension.end, { x: 80_000, y: 50_000 });
  const [t] = edit(b, ["dim_lead"], "Text", "NEW");
  assert.equal((t as unknown as { dimension: { override_text: unknown } }).dimension.override_text, "NEW");
  const [overridden] = edit(b, ["dim_a"], "Override Text", "X");
  assert.equal((overridden as unknown as { dimension: { override_text: unknown } }).dimension.override_text, "X");
  const [cleared] = edit(b, ["dim_lead"], "Text", "");
  assert.equal((cleared as unknown as { dimension: { override_text: unknown } }).dimension.override_text, null, "empty text is no override");
  assert.equal(edit(b, ["dim_a"], "Orientation", 30).length, 0, "the text follows the dimension: its angle is read-only");
  b.drawings!.dimensions[0] = dim({ keep_text_aligned: false, text_angle: 10_000 });
  assert.equal(rowOf(b, ["dim_a"], "Orientation")!.writable, true);
  assert.equal(rowOf(b, ["dim_a"], "Orientation")!.value, 10);
  assert.equal((edit(b, ["dim_a"], "Orientation", 30)[0] as { dimension: { text_angle: number } }).dimension.text_angle, 30_000);
});

test("a group is renamed with its members as they are", () => {
  const b = board();
  assert.deepEqual(edit(b, ["grp_a"], "Name", "core"), [{ op: "edit_group", id: "grp_a", name: "core", member_ids: ["U1", "trk_a"] }]);
});

test("one edit of several items is one command each, in selection order, and the ones that already have the value send nothing", () => {
  const b = board();
  assert.deepEqual(edit(b, ["trk_a", "trk_b", "trk_arc"], "Width", 300), [
    { op: "set_track_width", id: "trk_a", width: 300 },
    { op: "set_track_width", id: "trk_arc", width: 300 },
  ]);
  assert.deepEqual(edit(b, ["trk_a", "via_a", "zone_a"], "Net", "VCC"), [
    { op: "set_item_net", ids: ["trk_a"], net: "VCC" },
    { op: "set_item_net", ids: ["via_a"], net: "VCC" },
    { op: "set_item_net", ids: ["zone_a"], net: "VCC" },
  ]);
  assert.deepEqual(edit(b, ["U1", "R1", "txt_a"], "Locked", true), [
    { op: "set_locked", ids: ["U1"], locked: true },
    { op: "set_locked", ids: ["R1"], locked: true },
    { op: "set_locked", ids: ["txt_a"], locked: true },
  ]);
});

test("the choices a row offers: copper layers for copper, every layer for graphics, the board's nets", () => {
  const b = board();
  assert.deepEqual(rowOf(b, ["trk_a"], "Layer")!.choices!.map((c) => c.value), ["F.Cu", "B.Cu"]);
  const all = rowOf(b, ["txt_a"], "Layer")!.choices!.map((c) => c.value);
  assert.ok(all.includes("F.Cu") && all.includes("F.SilkS") && all.includes("Edge.Cuts"));
  assert.deepEqual(rowOf(b, ["trk_a"], "Net")!.choices!.map((c) => c.label), ["<no net>", "GND", "VCC"]);
  // A track and a text do not offer the same layers: no Layer row for a mix of them.
  assert.equal(rowOf(b, ["trk_a", "txt_a"], "Layer"), undefined);
  // Every layer something is drawn on is offered, even one the list does not know.
  const odd = board();
  odd.drawings!.texts[0] = { ...odd.drawings!.texts[0]!, layer: "User.7" };
  assert.ok(pcbContext(odd, "mm").allLayers.includes("User.7"));
});

test("pcbRow reads one row of a selection", () => {
  assert.deepEqual(pcbRow(board(), ["trk_a", "trk_b"], "Width", "mm"), { value: null, writable: true, choices: null });
  assert.equal(pcbRow(board(), ["trk_a", "txt_a"], "Net", "mm"), null, "a text has no net");
});

// ------------------------------------------------------------------------------------------------- footprints as editable objects

/** The board with what the backend sends of a footprint's fields, attributes and pads (crates/cli/src/fp_json.rs). */
const edited = (): BoardState => {
  const b = board();
  const u1 = b.parts.find((p) => p.ref === "U1")!;
  const field = (name: string, text: string, over: Record<string, unknown> = {}) => ({
    id: `U1:${name}`, name, text, x: 30_000, y: 16_900, angle: 0, w: 1000, h: 1000, thickness: 150, layer: "F.SilkS", visible: true, halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: true, knockout: false,
    lx: 0, ly: -3100, langle: 270_000, custom: false, ...over,
  });
  Object.assign(u1, {
    fields: [field("Reference", "U1"), field("Value", "MCU", { layer: "F.Fab", y: 21_000, ly: 1000 }), field("Vendor", "ACME", { layer: "F.Fab", visible: false, custom: true })],
    attrs: { kind: "smd", board_only: false, exclude_from_pos_files: false, exclude_from_bom: false, dnp: false, allow_missing_courtyard: false, custom: false },
  });
  const [p1, p2, p3] = u1.pads!;
  Object.assign(p1!, { id: "U1.1", shape: "rect", kind: "smd", size: [600, 600], rot: 0, ratio: null, drill: null, slot: null, edit: null });
  Object.assign(p2!, { id: "U1.2", shape: "circle", kind: "through_hole", size: [800, 800], rot: 0, ratio: null, drill: 400, slot: null, edit: null });
  Object.assign(p3!, { id: "U1.3", shape: "round_rect", kind: "smd", size: [800, 400], rot: 0, ratio: 0.25, drill: null, slot: null, edit: { number: "3", nth: 1, size: [800, 400] } });
  return b;
};

test("a field is an item of the board: REF:Name names it, and the caption says so", () => {
  const b = edited();
  assert.equal(pcbItemOf(b, "U1:Reference")?.type, "PCB_FIELD");
  assert.equal(pcbItemOf(b, "U1:Vendor")?.field?.text, "ACME");
  assert.equal(pcbItemOf(b, "U1:Nothing"), null);
  assert.equal(pcbItemOf(b, "R1:Reference"), null, "a footprint with no fields has none");
  assert.equal(pcbGrid(b, ["U1:Value"], "mm").caption, "Field");
});

test("a field: position, layer, orientation, then its text properties; a user field's text is its own", () => {
  const b = edited();
  assert.deepEqual(names(b, ["U1:Reference"]), [
    ["Basic Properties", ["Position X", "Position Y", "Layer", "Orientation"]],
    ["Text Properties", ["Text", "Thickness", "Italic", "Bold", "Mirrored", "Visible", "Width", "Height", "Horizontal Justification", "Vertical Justification", "Knockout", "Keep Upright"]],
  ]);
  assert.equal(rowOf(b, ["U1:Reference"], "Text")!.writable, false, "the reference comes from the schematic");
  assert.equal(rowOf(b, ["U1:Value"], "Text")!.writable, false);
  assert.equal(rowOf(b, ["U1:Vendor"], "Text")!.writable, true);
  assert.equal(rowOf(b, ["U1:Vendor"], "Visible")!.value, false);
  assert.equal(rowOf(b, ["U1:Reference"], "Width")!.value, 1000);
});

test("editing a field sends edit_board_field with its layout in the footprint's frame, and moves go through move_items", () => {
  const b = edited();
  const layout = {
    at: { x: 0, y: -3100 }, angle: 270_000, size: [1000, 1000], thickness: 150, layer: "F.SilkS", visible: true, halign: 0, valign: 0, mirror: false, bold: false, italic: false, keep_upright: true, knockout: false,
  };
  assert.deepEqual(edit(b, ["U1:Reference"], "Height", 1200), [{ op: "edit_board_field", part: "U1", name: "Reference", layout: { ...layout, size: [1000, 1200] } }]);
  assert.deepEqual(edit(b, ["U1:Reference"], "Thickness", 200), [{ op: "edit_board_field", part: "U1", name: "Reference", layout: { ...layout, thickness: 200 } }]);
  assert.deepEqual(edit(b, ["U1:Reference"], "Layer", "F.Fab"), [{ op: "edit_board_field", part: "U1", name: "Reference", layout: { ...layout, layer: "F.Fab" } }]);
  assert.deepEqual(edit(b, ["U1:Reference"], "Horizontal Justification", "left"), [{ op: "edit_board_field", part: "U1", name: "Reference", layout: { ...layout, halign: -1 } }]);
  assert.deepEqual(edit(b, ["U1:Reference"], "Vertical Justification", "bottom"), [{ op: "edit_board_field", part: "U1", name: "Reference", layout: { ...layout, valign: 1 } }]);
  assert.deepEqual(edit(b, ["U1:Reference"], "Mirrored", true), [{ op: "edit_board_field", part: "U1", name: "Reference", layout: { ...layout, mirror: true } }]);
  assert.deepEqual(edit(b, ["U1:Reference"], "Bold", true), [{ op: "edit_board_field", part: "U1", name: "Reference", layout: { ...layout, bold: true } }]);
  assert.deepEqual(edit(b, ["U1:Vendor"], "Text", "Other"), [{ op: "edit_board_field", part: "U1", name: "Vendor", layout: { ...layout, layer: "F.Fab", visible: false }, text: "Other" }]);
  assert.deepEqual(edit(b, ["U1:Reference"], "Text", "R9"), [], "a reference is not edited here");
  assert.deepEqual(edit(b, ["U1:Reference"], "Position X", 31_000), [{ op: "move_items", ids: ["U1:Reference"], dx: 1_000, dy: 0 }]);
  assert.deepEqual(edit(b, ["U1:Reference"], "Position Y", 16_900), [], "already there");
});

test("a field's orientation is its absolute angle: the footprint's own turn is taken back out", () => {
  const b = edited();
  // U1 is turned 90 degrees clockwise (rot 90); the field reads at 0 on the board when its own angle is 270 - 0 turns... board angle = langle - rot = 270000 - 90000 = 180000?
  // The fixture says the field's board angle is 0, so a new board angle of 45 degrees is langle = 45000 + 90000.
  const [cmd] = edit(b, ["U1:Reference"], "Orientation", 45);
  assert.equal(cmd!.op, "edit_board_field");
  assert.equal((cmd as Extract<Cmd, { op: "edit_board_field" }>).layout!.angle, 135_000);
  // A bottom-side footprint's frame is mirrored, and so is the sense of its angles.
  const bottom = edited();
  Object.assign(bottom.parts.find((p) => p.ref === "U1")!, { side: "bottom" });
  const [cmd2] = edit(bottom, ["U1:Reference"], "Orientation", 45);
  assert.equal((cmd2 as Extract<Cmd, { op: "edit_board_field" }>).layout!.angle, 360_000 - 135_000);
});

test("a footprint lists its attributes and each is an edit_board_footprint with the others as they were", () => {
  const b = edited();
  assert.deepEqual(names(b, ["U1"]).map(([g]) => g), ["Basic Properties", "Fields", "Footprint Properties", "Attributes", "Overrides", "Placement"]);
  assert.deepEqual(names(b, ["U1"]).find(([g]) => g === "Attributes")![1], ["Component Type", "Not in Schematic", "Exclude From Position Files", "Exclude From Bill of Materials", "Do not Populate"]);
  assert.deepEqual(names(b, ["U1"]).find(([g]) => g === "Overrides")![1], ["Exempt From Courtyard Requirement"]);
  const all = { kind: "smd", board_only: false, exclude_from_pos_files: false, exclude_from_bom: false, dnp: false, allow_missing_courtyard: false };
  assert.deepEqual(edit(b, ["U1"], "Do not Populate", true), [{ op: "edit_board_footprint", part: "U1", attrs: { ...all, dnp: true } }]);
  assert.deepEqual(edit(b, ["U1"], "Exclude From Position Files", true), [{ op: "edit_board_footprint", part: "U1", attrs: { ...all, exclude_from_pos_files: true } }]);
  assert.deepEqual(edit(b, ["U1"], "Exclude From Bill of Materials", true), [{ op: "edit_board_footprint", part: "U1", attrs: { ...all, exclude_from_bom: true } }]);
  assert.deepEqual(edit(b, ["U1"], "Not in Schematic", true), [{ op: "edit_board_footprint", part: "U1", attrs: { ...all, board_only: true } }]);
  assert.deepEqual(edit(b, ["U1"], "Exempt From Courtyard Requirement", true), [{ op: "edit_board_footprint", part: "U1", attrs: { ...all, allow_missing_courtyard: true } }]);
  assert.deepEqual(edit(b, ["U1"], "Component Type", "through_hole"), [{ op: "edit_board_footprint", part: "U1", attrs: { ...all, kind: "through_hole" } }]);
  assert.deepEqual(edit(b, ["U1"], "Do not Populate", false), [], "already so");
  // Several footprints: one command each.
  const two = edited();
  Object.assign(two.parts.find((p) => p.ref === "R1")!, { attrs: { ...all, custom: false }, fields: [] });
  assert.equal(edit(two, ["U1", "R1"], "Do not Populate", true).length, 2);
});

test("a pad: type, shape, size, corner radius and hole are edit_board_pad commands that keep the edit the pad has", () => {
  const b = edited();
  const pad = (patch: Record<string, unknown>, number = "1") => [{ op: "edit_board_pad", part: "U1", edit: { number, nth: 1, ...patch } }];
  assert.deepEqual(edit(b, ["U1.1"], "Pad Shape", "oval"), pad({ shape: "oval" }));
  assert.deepEqual(edit(b, ["U1.1"], "Size X", 700), pad({ size: [700, 600] }));
  assert.deepEqual(edit(b, ["U1.1"], "Size Y", 650), pad({ size: [600, 650] }));
  assert.deepEqual(edit(b, ["U1.2"], "Size X", 900), pad({ size: [900, 900] }, "2"), "a circle stays round");
  // The edit that is already stored comes back with the new change laid over it.
  assert.deepEqual(edit(b, ["U1.3"], "Corner Radius Ratio", 0.4), pad({ size: [800, 400], roundrect_ratio: 0.4 }, "3"));
  assert.deepEqual(edit(b, ["U1.3"], "Corner Radius Size", 100), [], "100 um on a 400 um side is the 25% it already has");
  assert.deepEqual(edit(b, ["U1.3"], "Corner Radius Size", 120), pad({ size: [800, 400], roundrect_ratio: 0.3 }, "3"));
  assert.equal(rowOf(b, ["U1.3"], "Corner Radius Size")!.value, 100);
  assert.equal(rowOf(b, ["U1.1"], "Corner Radius Ratio"), undefined, "only a rounded rectangle has a radius");
  // The hole: size, and round or oblong.
  assert.deepEqual(edit(b, ["U1.2"], "Hole Size X", 450), pad({ drill: 450 }, "2"));
  assert.deepEqual(edit(b, ["U1.2"], "Hole Shape", "oblong"), pad({ drill_slot: [400, 600] }, "2"));
  assert.deepEqual(edit(b, ["U1.1"], "Hole Size X", 300), [], "an SMD pad has no hole to edit");
  // Making an SMD pad through-hole gives it a hole to start from: half its smaller side.
  assert.deepEqual(edit(b, ["U1.1"], "Pad Type", "through_hole"), pad({ kind: "through_hole", drill: 300 }));
  // Several pads: one command each.
  assert.equal(edit(b, ["U1.1", "U1.3"], "Size X", 700).length, 2);
});
