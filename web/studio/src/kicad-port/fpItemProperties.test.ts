// The Footprint Editor's Properties pane against KiCad's own registrations (UI-LEFT Y3): for every class the pane lists, the rows of the studio's registry
// (kicad-port/fpItemProperties.ts) are the rows of KiCad's `PROPERTY_MANAGER` table (kicad-port/pcbKicadRegistrations.ts, the one the board's panel is tested against),
// as the library editor's panel lists them (`isLibraryEditor`: the Net, Pin Name, Pin Type and Component Class rows left out). The rows' behaviour is tested below.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd, LibraryFootprint } from "../api/types";
import { registeredRows } from "./propertyGrid";
import { kicadPcbRegistry } from "./pcbKicadRegistrations";
import { FP_PROPERTIES, fpEdit, fpGrid, type FpPropCtx } from "./fpItemProperties";

for (const type of ["FOOTPRINT", "PAD", "PCB_SHAPE", "PCB_TEXT", "PCB_FIELD"]) {
  test(`${type}: the Footprint Editor's rows are KiCad's, in KiCad's groups and order, as a library editor lists them`, () => {
    assert.deepEqual(registeredRows(FP_PROPERTIES, type, { libraryEditor: true }), registeredRows(kicadPcbRegistry(), type, { libraryEditor: true }));
  });
}

test("the library editor leaves out the rows hidden from it", () => {
  const pad = registeredRows(kicadPcbRegistry(), "PAD", { libraryEditor: true });
  assert.ok(!pad.includes("/Net") && !pad.includes("Pad Properties/Pin Name") && !pad.includes("Pad Properties/Pin Type") && pad.includes("Pad Properties/Pad Number"));
  assert.ok(!registeredRows(kicadPcbRegistry(), "FOOTPRINT", { libraryEditor: true }).includes("Footprint Properties/Component Class"));
  assert.ok(registeredRows(kicadPcbRegistry(), "PAD").includes("/Net"), "a design editor shows the net");
});

const fp: LibraryFootprint = {
  name: "Lib:FP",
  description: "a part",
  keywords: "",
  attributes: { smd: true, through_hole: false, exclude_from_bom: false, exclude_from_position_files: false, board_only: false, dnp: false, allow_missing_courtyard: false, allow_soldermask_bridges: false },
  reference_visible: true,
  value_visible: true,
  model: null,
  zone_connection: null,
  pads: [{ id: "p1", number: "1", at: { x: 1_000_000, y: 0 }, size: [800_000, 600_000], offset: { x: 0, y: 0 }, shape: "rect", kind: "smd", drill: null, drill_slot: null, rot: 0, layers: ["F.Cu", "F.Paste", "F.Mask"] }],
  graphics: [{ id: "g1", kind: "rect", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: -2_000_000, y: -1_000_000 }, end: { x: 2_000_000, y: 1_000_000 } }],
  texts: [{ id: "t1", content: "X", at: { x: 0, y: 2_000_000 }, angle: 0, layer: "F.Fab", size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false }],
  fields: [],
  courtyard: null,
  anchor: { x: 0, y: 0 },
  published: false,
} as unknown as LibraryFootprint;
const ctx: FpPropCtx = { fp, name: "Lib:FP", units: "mm" };
const row = (ids: string[], name: string) => fpGrid(ctx, ids).groups.flatMap((g) => g.rows).find((r) => r.name === name);
const only = (ids: string[], name: string, value: string | number | boolean): Cmd => {
  const plan = fpEdit(ctx, ids, name, value);
  assert.ok(plan.ok, JSON.stringify(plan));
  assert.equal(plan.cmds.length, 1, JSON.stringify(plan.cmds));
  return plan.cmds[0]!;
};

test("with nothing selected the pane shows the footprint (`getSelection`'s fallback), and edits its properties and settings", () => {
  const g = fpGrid(ctx, []);
  assert.equal(g.caption, "Footprint");
  assert.equal(row([], "Library Description")?.value, "a part");
  const c = only([], "Library Description", "another") as Extract<Cmd, { op: "edit_footprint_properties" }>;
  assert.equal(c.op, "edit_footprint_properties");
  assert.equal(c.description, "another");
  const s = only([], "Clearance Override", 200_000) as { op: string; verb: string; settings: { clearance: number } };
  assert.deepEqual([s.op, s.verb, s.settings.clearance], ["fp_editor", "set_settings", 200_000]);
});

test("a pad's rows read the pad and edit it whole (`edit_pad`)", () => {
  assert.equal(row(["p1"], "Size X")?.value, 800_000);
  const c = only(["p1"], "Size Y", 900_000) as Extract<Cmd, { op: "edit_pad" }>;
  assert.deepEqual(c.pad.size, [800_000, 900_000]);
  const fab = only(["p1"], "Fabrication Property", "pad_prop_testpoint") as Extract<Cmd, { op: "edit_pad" }>;
  assert.equal((fab.pad.kicad as { property?: string }).property, "pad_prop_testpoint");
  assert.equal(row(["p1"], "Pad Type")?.writable, true);
  assert.ok(!row(["p1"], "Net"), "no Net row in the Footprint Editor");
  const move = only(["p1"], "Position X", 1_500_000) as { op: string; verb: string; dx: number };
  assert.deepEqual([move.op, move.verb, move.dx], ["fp_item", "move", 500_000]);
});

test("a shape's and a text's rows edit them with their looks", () => {
  const style = only(["g1"], "Line Style", "dash") as { verb: string; look: { line_style: string } };
  assert.deepEqual([style.verb, style.look.line_style], ["set_graphic", "dash"]);
  const fill = only(["g1"], "Fill", "hatch") as { look: { hatch: string }; shape: { filled: boolean } };
  assert.deepEqual([fill.look.hatch, fill.shape.filled], ["hatch", false]);
  const w = only(["g1"], "Width", 3_000_000) as { shape: { end: { x: number } } };
  assert.equal(w.shape.end.x, 1_000_000);
  const bold = only(["t1"], "Bold", true) as { verb: string; look: { bold: boolean } };
  assert.deepEqual([bold.verb, bold.look.bold], ["set_text", true]);
  const field = only(["field:Value"], "Height", 1_500_000) as { verb: string; layout: { size: [number, number] } };
  assert.deepEqual([field.verb, field.layout.size[1]], ["edit_field", 1_500_000]);
});

test("Pad Type is `PAD::SetAttribute`, Pad Shape `SetFrontShape`, Hole Shape `SetDrillShape`, Copper Layers `SetUnconnectedLayerMode` (UI-LEFT-2 FF3)", () => {
  const th = only(["p1"], "Pad Type", "through_hole") as Extract<Cmd, { op: "edit_pad" }>;
  assert.equal(th.pad.kind, "through_hole");
  assert.deepEqual(th.pad.layers, ["*.Cu", "F.Paste", "F.Mask"], "plumped up to every copper layer");
  assert.equal(th.pad.drill, 300_000, "the studio's pad gets a hole: half its smaller side");
  const conn = only(["p1"], "Pad Type", "conn") as Extract<Cmd, { op: "edit_pad" }>;
  assert.deepEqual([conn.pad.kind, (conn.pad.kicad as { connector?: boolean }).connector], ["smd", true]);
  const np = only(["p1"], "Pad Type", "non_plated_hole") as Extract<Cmd, { op: "edit_pad" }>;
  assert.equal(np.pad.number, "", "an NPTH pad has no number");
  const rr = only(["p1"], "Pad Shape", "round_rect") as Extract<Cmd, { op: "edit_pad" }>;
  assert.equal(rr.pad.roundrect_ratio, 0.25, "the IPC rounding: a quarter of 0.6 mm is under 0.25 mm");
  const cu = only(["p1"], "Copper Layers", "remove_all") as Extract<Cmd, { op: "edit_pad" }>;
  assert.equal(cu.pad.stack?.unconnected, "remove_all");
  assert.equal(row(["p1"], "Hole Shape")?.writable, false, "an SMD pad can have no hole (`padCanHaveHole`)");
});

test("a pad's backdrill, post-machining and teardrops are the file's, set by `set_pad_extras`; a line's endings by its looks (UI-LEFT-2 FF3)", () => {
  const thPad = { ...fp.pads[0]!, id: "p2", kind: "through_hole", drill: 800_000, layers: ["*.Cu", "*.Mask"] };
  const f2 = { ...fp, pads: [thPad], graphics: [{ id: "l1", kind: "segment", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: 0, y: 0 }, end: { x: 1_000_000, y: 0 } }], pad_extras: { p2: { drill_props: {}, teardrops: null } } } as unknown as LibraryFootprint;
  const c2: FpPropCtx = { fp: f2, name: "Lib:FP", units: "mm" };
  const one = (ids: string[], name: string, value: string | number | boolean) => {
    const plan = fpEdit(c2, ids, name, value);
    assert.ok(plan.ok && plan.cmds.length === 1, JSON.stringify(plan));
    return plan.cmds[0] as unknown as Record<string, unknown>;
  };
  const bd = one(["p2"], "Backdrill Mode", "bottom");
  assert.deepEqual([bd.verb, (bd.drill_props as { backdrill?: unknown }).backdrill !== undefined], ["set_pad_extras", true]);
  const pm = one(["p2"], "Top Post-machining", "counterbore");
  assert.equal(pm.verb, "set_pad_extras");
  const td = one(["p2"], "Enable Teardrops", true);
  assert.deepEqual([td.verb, (td.teardrops as { enabled: boolean }).enabled], ["set_pad_extras", true]);
  const end = one(["l1"], "End Shape", 1);
  assert.equal(end.verb, "set_graphic");
  assert.equal((end.look as { endings: { end: { style: string } } }).endings.end.style, "arrow");
});
