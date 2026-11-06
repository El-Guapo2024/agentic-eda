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

test("a pad's rows read the pad and edit it whole (`edit_pad`), and the rows the dialog sets are read-only", () => {
  assert.equal(row(["p1"], "Size X")?.value, 800_000);
  const c = only(["p1"], "Size Y", 900_000) as Extract<Cmd, { op: "edit_pad" }>;
  assert.deepEqual(c.pad.size, [800_000, 900_000]);
  const fab = only(["p1"], "Fabrication Property", "pad_prop_testpoint") as Extract<Cmd, { op: "edit_pad" }>;
  assert.equal((fab.pad.kicad as { property?: string }).property, "pad_prop_testpoint");
  assert.equal(row(["p1"], "Pad Type")?.writable, false);
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
