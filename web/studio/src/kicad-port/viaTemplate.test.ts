import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { TEMPLATE_VIA_ID, templateAsVia, templateBoard, templateFromDialog, templateLocked, templateRings, templateViaType, viaTemplateForm, viaTemplateFrom, withViaType, type StitchVia } from "./viaTemplate";
import { trackViaForm } from "./trackViaDialog";

const COPPER = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
const read: StitchVia = {
  size_nm: 600_000,
  drill_nm: 300_000,
  from_layer: "F.Cu",
  to_layer: "In2.Cu",
  text: '(via blind (at 0 0) (size 0.6) (drill 0.3) (layers "F.Cu" "In2.Cu") (remove_unused_layers yes) (keep_end_layers yes) (tenting (front yes) (back yes)) (free yes) (net "") (uuid "t"))',
};

test("the template's type and annular rings are its file's until Configure sets them", () => {
  assert.equal(templateViaType(read), "blind");
  assert.equal(templateRings(read), "remove_except_start_and_end");
  assert.equal(templateViaType({ ...read, text: undefined }), "through");
  assert.equal(templateRings({ ...read, text: undefined }), "keep_all");
  assert.equal(templateViaType({ ...read, kind: "micro" }), "micro");
  assert.equal(templateRings({ ...read, annular: "start_end_only" }), "start_end_only");
});

test("OK on the template: ValidateViaParameters' words, a through via on the outer layers, and only what changed over the text", () => {
  const f = viaTemplateForm(read);
  assert.deepEqual(viaTemplateFrom(read, f, COPPER), { via: read }, "opened and confirmed: as it was");
  assert.deepEqual(viaTemplateFrom(read, { ...f, drillNm: 600_000 }, COPPER), { error: "Via hole size must be smaller than via diameter" });
  assert.deepEqual(viaTemplateFrom(read, { ...f, diameterNm: null }, COPPER), { error: "No via diameter defined." });
  assert.deepEqual(viaTemplateFrom(read, { ...f, to: "F.Cu" }, COPPER), { error: "Via start layer and end layer cannot be the same" });
  const through = withViaType(f, "through", COPPER);
  assert.deepEqual([through.from, through.to], ["F.Cu", "B.Cu"], "onViaEdit");
  const out = viaTemplateFrom(read, { ...through, rings: "remove_all", diameterNm: 800_000, drillNm: 400_000 }, COPPER);
  assert.ok("via" in out);
  assert.deepEqual(
    [out.via.kind, out.via.annular, out.via.from_layer, out.via.to_layer, out.via.size_nm, out.via.drill_nm],
    ["through", "remove_all", "F.Cu", "B.Cu", 800_000, 400_000]
  );
  assert.equal(out.via.text, read.text, "the rest of the file's via stays");
});

test("Configure is Track & Via Properties on the template: its other rows -- net, lock, protection, backdrill, teardrops -- set over the text (UI-LEFT-2 PP22)", () => {
  const view = { props: { front_tenting: true, back_tenting: true }, teardrops: null, net: "GND", locked: false, type: "blind" as const, unconnected: "remove_except_start_and_end" as const };
  const board = { parts: [], locked: ["V9"], routing: { tracks: [{ id: "T1" }], vias: [{ id: "V9" }], zones: [], track_width_presets: [], via_presets: [] } } as unknown as BoardState;
  const via = templateAsVia(read, view);
  assert.deepEqual([via.id, via.net, via.type, via.unconnected, via.d, via.drill, via.props], [TEMPLATE_VIA_ID, "GND", "blind", "remove_except_start_and_end", 600_000, 300_000, view.props]);
  const one = templateBoard(board, via, templateLocked(read, view));
  assert.deepEqual([one.routing!.tracks.length, one.routing!.vias.length, one.locked], [0, 1, []], "the template is the board's one via, unlocked");
  const before = trackViaForm(one, [TEMPLATE_VIA_ID]);
  assert.deepEqual(templateFromDialog(read, one, before, before, COPPER), { via: read }, "opened and confirmed: as it was");
  const changed = { ...before, net: "VCC", locked: true, protection: "ia" as const, teardrops: { ...before.teardrops, enabled: true } };
  const out = templateFromDialog(read, one, before, changed, COPPER);
  assert.ok("via" in out);
  assert.equal(out.via.net, "VCC");
  assert.equal(out.via.locked, true);
  assert.equal(out.via.teardrops?.enabled, true);
  assert.ok(out.via.props, "the protection preset is in the template's props");
  assert.deepEqual(templateFromDialog(read, one, before, { ...before, drill: 600_000 }, COPPER), { error: "Via hole size must be smaller than via diameter" });
});
