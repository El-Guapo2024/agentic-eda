import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DIM_EXTENSION_HEIGHT_NM,
  FP_RULE_AREA_KEEPOUTS,
  fpAddCmd,
  fpDimensionClick,
  fpDimensionMotion,
  fpDimensionPreview,
  fpNewBarcode,
  fpNewDimension,
  fpNewImage,
  fpNewPoint,
  fpNewRuleArea,
  fpNewTable,
  fpPatchTable,
  fpSetCmd,
  fpZoneWithSettings,
  radialKnee,
  type FpDimDraw,
} from "./fpDrawTools";
import type { CmdZone, ZoneSettingsFields } from "../api/types";

const MM = 1_000_000;

test("DrawDimension: an aligned dimension takes three clicks -- origin, end, height -- and is the editor's style on the active layer", () => {
  let r = fpDimensionClick(null, "aligned", "F.Fab", [0, 0], true);
  assert.equal(r.commit, null);
  assert.equal(r.draw?.step, "end");
  const d0 = r.draw!.dim;
  // the Fab class's text (1 mm, 0.15 mm) and line (0.1 mm); 50 mil arrows, 0.5 mm extension offset, `m_arrowLength * sin( 27.5 deg )` extension height
  assert.deepEqual([d0.text_size_nm, d0.text_thickness_nm, d0.stroke_width, d0.arrow_length, d0.extension_offset, d0.extension_height], [MM, 150_000, 100_000, 1_270_000, 500_000, DIM_EXTENSION_HEIGHT_NM]);
  assert.equal(DIM_EXTENSION_HEIGHT_NM, 586_420);
  assert.deepEqual([d0.units, d0.units_format, d0.precision, d0.suppress_trailing_zeros, d0.text_position, d0.keep_text_aligned], ["automatic", "no_suffix", 4, true, "outside", true]);
  // the same point again is no dimension: the tool stays at SET_END
  r = fpDimensionClick(r.draw, "aligned", "F.Fab", [0, 0], true);
  assert.equal(r.draw?.step, "end");
  r = fpDimensionClick(r.draw, "aligned", "F.Fab", [4 * MM, 0], true);
  assert.equal(r.draw?.step, "height");
  assert.equal(r.commit, null);
  r = fpDimensionClick(r.draw, "aligned", "F.Fab", [3 * MM, -2 * MM], true);
  assert.equal(r.draw, null);
  assert.deepEqual(r.commit?.kind, { kind: "aligned", height: -2 * MM });
  assert.deepEqual([r.commit?.start, r.commit?.end], [{ x: 0, y: 0 }, { x: 4 * MM, y: 0 }]);
});

test("constrainDimension: the end is held to 45 degrees in the 45- and 90-degree modes (not with Ctrl), and always for a centre mark", () => {
  const start = fpDimensionClick(null, "aligned", "F.SilkS", [0, 0], true).draw!;
  assert.deepEqual(fpDimensionMotion(start, [3 * MM, 1 * MM], true).dim.end, { x: 3 * MM, y: 0 });
  assert.deepEqual(fpDimensionMotion(start, [3 * MM, 1 * MM], false).dim.end, { x: 3 * MM, y: MM });
  const centre = fpDimensionClick(null, "center", "F.SilkS", [0, 0], false).draw!;
  assert.deepEqual(fpDimensionMotion(centre, [3 * MM, 1 * MM], false).dim.end, { x: 3 * MM, y: 0 });
  // a centre mark is done at its second click, with its constructor's override text and no suffix
  const done = fpDimensionClick(centre, "center", "F.SilkS", [2 * MM, 2 * MM], false);
  assert.equal(done.draw, null);
  assert.deepEqual(done.commit?.end, { x: 2 * MM, y: 2 * MM });
  assert.equal(done.commit?.override_text, "");
});

test("orthogonal: the longer side is measured at SET_END; the orientation follows the cursor out of the box at SET_HEIGHT", () => {
  let draw: FpDimDraw = fpDimensionClick(null, "orthogonal", "F.Fab", [0, 0], false).draw!;
  draw = fpDimensionMotion(draw, [4 * MM, 1 * MM], false);
  assert.equal(draw.dim.kind.kind === "orthogonal" && draw.dim.kind.horizontal, true);
  draw = fpDimensionMotion(draw, [1 * MM, 4 * MM], false);
  assert.equal(draw.dim.kind.kind === "orthogonal" && draw.dim.kind.horizontal, false);
  draw = fpDimensionClick(fpDimensionMotion(draw, [4 * MM, 1 * MM], false), "orthogonal", "F.Fab", [4 * MM, 1 * MM], false).draw!;
  assert.equal(draw.step, "height");
  // above the box, between its sides: horizontal, the height the cursor's y from the start
  const h = fpDimensionMotion(draw, [2 * MM, -3 * MM], false);
  assert.deepEqual(h.dim.kind, { kind: "orthogonal", height: -3 * MM, horizontal: true });
  // right of the box, between its top and bottom: vertical, the height the cursor's x from the start
  const v = fpDimensionMotion(draw, [6 * MM, 0.5 * MM], false);
  assert.deepEqual(v.dim.kind, { kind: "orthogonal", height: 6 * MM, horizontal: false });
  // inside the box the orientation stays
  assert.equal(fpDimensionMotion(v, [2 * MM, 0.5 * MM], false).dim.kind.kind === "orthogonal" && (fpDimensionMotion(v, [2 * MM, 0.5 * MM], false).dim.kind as { horizontal: boolean }).horizontal, false);
});

test("radial and leader: done at the second click, the text past the knee or the end, `R ` before a radius", () => {
  const radial = fpDimensionClick(null, "radial", "F.Fab", [0, 0], false).draw!;
  assert.equal(radial.dim.prefix, "R ");
  assert.deepEqual(radial.dim.kind, { kind: "radial", leader_length: 3 * 1_270_000 });
  const r = fpDimensionClick(radial, "radial", "F.Fab", [2 * MM, 0], false);
  assert.equal(r.draw, null);
  const knee = radialKnee(r.commit!, 3 * 1_270_000);
  assert.deepEqual(knee, [2 * MM + 3_810_000, 0]);
  assert.deepEqual(r.commit?.text_pos, { x: knee[0] + 12_700_000, y: 0 });
  const leader = fpDimensionClick(null, "leader", "F.Fab", [0, 0], false);
  assert.deepEqual(leader.draw?.dim.text_pos, { x: 0, y: 0 });
  assert.equal(leader.draw?.dim.override_text, "");
  assert.equal(leader.draw?.dim.keep_text_aligned, false);
  const l = fpDimensionClick(leader.draw, "leader", "F.Fab", [-2 * MM, -2 * MM], false);
  assert.deepEqual(l.commit?.text_pos, { x: -2 * MM - 12_700_000, y: -2 * MM });
  assert.equal(fpDimensionPreview(l.commit!).length, 2);
});

test("the preview of an aligned dimension is its crossbar and its two extension lines", () => {
  const dim = { ...fpNewDimension("aligned", "F.Fab", [0, 0]), end: { x: 4 * MM, y: 0 }, kind: { kind: "aligned" as const, height: MM } };
  assert.deepEqual(fpDimensionPreview(dim), [
    [[0, 0], [0, MM]],
    [[4 * MM, 0], [4 * MM, MM]],
    [[0, MM], [4 * MM, MM]],
  ]);
});

test("DrawZone( ruleArea ): the outline on the active layer, no net, ZONE_SETTINGS' keepouts; its dialog's settings go onto the IR zone", () => {
  const settings = { clearance: 500_000, min_thickness: 250_000 } as ZoneSettingsFields;
  const z = fpNewRuleArea("F.Cu", [[0, 0], [MM, 0], [MM, MM]], settings);
  assert.equal(z.net, "");
  assert.equal(z.is_rule_area, true);
  assert.deepEqual([z.keepout_tracks, z.keepout_vias, z.keepout_pads, z.keepout_copper_pour, z.keepout_footprints], [true, true, true, false, false]);
  assert.deepEqual(FP_RULE_AREA_KEEPOUTS.keepout_footprints, false);
  assert.deepEqual(z.outline, [{ x: 0, y: 0 }, { x: MM, y: 0 }, { x: MM, y: MM }]);
  // two layers: the set; a rule area keeps no name; a zone that is no rule area does
  const edited = fpZoneWithSettings({ ...z, id: "zone_1" } as CmdZone, { is_rule_area: true, keepout_footprints: true }, ["F.Cu", "B.Cu"], "ignored");
  assert.deepEqual([edited.layer, (edited as { layers?: string[] }).layers, (edited as { name?: string }).name, edited.keepout_footprints, edited.id], ["F.Cu", ["F.Cu", "B.Cu"], undefined, true, "zone_1"]);
  const plain = fpZoneWithSettings({ ...z, layers: ["F.Cu", "B.Cu"] } as unknown as CmdZone, { is_rule_area: false }, ["F.SilkS"], "Keep");
  assert.deepEqual([plain.layer, (plain as { layers?: string[] }).layers, (plain as { name?: string }).name], ["F.SilkS", undefined, "Keep"]);
  // the outline display and its pitch go onto the zone; an offsets grid left unchanged (none sent) keeps the zone's own
  const hatched = { ...z, hatch_offsets: { "F.Cu": { x_nm: MM, y_nm: 0 } } } as unknown as CmdZone;
  const styled = fpZoneWithSettings(hatched, { is_rule_area: true, border_style: "full", border_hatch_pitch: 2 * MM } as never, ["F.Cu"], "");
  assert.deepEqual([styled.border_style, styled.border_hatch_pitch, styled.hatch_offsets], ["full", 2 * MM, { "F.Cu": { x_nm: MM, y_nm: 0 } }]);
});

test("PlacePoint, DrawBarcode and PlaceReferenceImage make their item at the click on the active layer", () => {
  assert.deepEqual(fpNewPoint("F.Fab", [1.4, 2.6]), { layer: "F.Fab", at: { x: 1, y: 3 }, size_nm: MM });
  const b = fpNewBarcode("F.SilkS", [MM, 2 * MM]);
  assert.deepEqual([b.layer, b.at, b.kind, b.ecc, b.width_nm, b.text_height_nm], ["F.SilkS", { x: MM, y: 2 * MM }, "qr", "L", 40 * MM, MM]);
  assert.deepEqual(fpNewImage("Cmts.User", [0, MM], "iVBOR"), { layer: "Cmts.User", at: { x: 0, y: MM }, scale_ppm: 1_000_000, data: "iVBOR" });
});

test("the verbs: one fp_item add, one fp_item set", () => {
  const item = { point: fpNewPoint("F.Fab", [0, 0]) };
  assert.deepEqual(fpAddCmd("Lib:X", item), { op: "fp_item", verb: "add", footprint: "Lib:X", item });
  assert.deepEqual(fpSetCmd("Lib:X", "pnt_1", item), { op: "fp_item", verb: "set", footprint: "Lib:X", id: "pnt_1", item });
});

test("DrawTable: the board's new grid (Table::new_grid), then the dialog's patch (patch_table)", () => {
  const t = fpNewTable({ layer: "B.SilkS", at: [MM, 2 * MM], cols: 2, rows: 3, cellW: 5 * MM, cellH: 2 * MM, lineWidth: 100_000 });
  assert.deepEqual([t.col_widths, t.row_heights, t.cells.length], [[5 * MM, 5 * MM], [2 * MM, 2 * MM, 2 * MM], 6]);
  assert.deepEqual(t.cells[0]!.margins, [1_002_500, 1_002_500, 1_002_500, 1_002_500]);
  assert.equal(t.cells[0]!.style.mirror, true); // a back layer's cells are mirrored
  assert.deepEqual(t.border, { external: true, header: true, stroke: { width_nm: 100_000, style: "default" } });
  const p = fpPatchTable(t, {
    texts: [["a", "b"], ["c", "d"], ["e", "f"]],
    border: { external: false, header: false, width_nm: 200_000, style: "dash" },
    separators: { rows: true, cols: false, width_nm: 150_000, style: "default" },
  })!;
  assert.deepEqual(p.cells.map((c) => c.text), ["a", "b", "c", "d", "e", "f"]);
  assert.deepEqual(p.border.stroke, { width_nm: -1, style: "dash" }); // no border drawn: no stroke
  assert.deepEqual(p.separators, { rows: true, cols: false, stroke: { width_nm: 150_000, style: "solid" } });
  assert.equal(t.cells[0]!.text, ""); // a copy
  assert.equal(fpPatchTable(t, { texts: [["a"]] }), null);
  // a covered cell has no text
  const merged = { ...t, cells: t.cells.map((c, i) => (i === 1 ? { ...c, col_span: 0 } : c)) };
  assert.equal(fpPatchTable(merged, { texts: [["a", "b"], ["c", "d"], ["e", "f"]] })!.cells[1]!.text, "");
});
