import { test } from "node:test";
import assert from "node:assert/strict";
import { PAD_DLG_TYPES, padDlgTypeOf, padTypeHas, padTypeSelected, setPadAttribute } from "./padType";
import type { LibraryPad } from "../api/types";

const pad = (over: Partial<LibraryPad> = {}): LibraryPad => ({
  id: "p",
  number: "1",
  at: { x: 0, y: 0 },
  offset: { x: 0, y: 0 },
  size: [1_600_000, 1_600_000],
  shape: "circle",
  kind: "through_hole",
  drill: 800_000,
  drill_slot: null,
  rot: 0,
  roundrect_ratio: null,
  trapezoid_delta: null,
  chamfer_ratio: null,
  chamfer_corners: { top_left: false, top_right: false, bottom_left: false, bottom_right: false },
  layers: ["*.Cu", "F.Mask", "B.Mask"],
  clearance_override: null,
  thermal_gap_override: null,
  thermal_spoke_width_override: null,
  ...over,
});

test("the five pad types, KiCad's words (m_padTypeChoices)", () => {
  assert.deepEqual(
    PAD_DLG_TYPES.map((t) => t.label),
    ["Through-hole", "SMD", "Edge Connector", "NPTH, Mechanical", "SMD Aperture"]
  );
});

test("PadTypeSelected: a through-hole pad set to SMD takes SMDMask and loses its hole (not an SMD pad on every copper layer)", () => {
  const th = pad();
  const smd = padTypeSelected(th, "smd", th);
  assert.equal(smd.kind, "smd");
  assert.deepEqual(smd.layers, ["F.Cu", "F.Paste", "F.Mask"]);
  assert.equal(smd.drill, null);
  assert.equal(smd.drill_slot, null);
  assert.equal(smd.number, "1");
});

test("PadTypeSelected: NPTH has no number and the number box is disabled; going back to a connected type gives the pad's number back", () => {
  const th = pad();
  const npth = padTypeSelected(th, "npth", th);
  assert.equal(npth.kind, "non_plated_hole");
  assert.equal(npth.number, "");
  assert.deepEqual(npth.layers, ["*.Cu", "*.Mask"]);
  assert.equal(npth.drill, 800_000, "the hole stays");
  assert.equal(padTypeHas("npth").connection, false);
  const back = padTypeSelected(npth, "pth", th);
  assert.equal(back.number, "1", "`m_padNumCtrl->ChangeValue( m_currentPad->GetNumber() )`");
});

test("PadTypeSelected: Edge Connector is an SMD pad with the connector flag on ConnSMDMask; another type clears the flag", () => {
  const smd = pad({ kind: "smd", drill: null, layers: ["F.Cu", "F.Paste", "F.Mask"] });
  const conn = padTypeSelected(smd, "conn", smd);
  assert.equal(conn.kind, "smd");
  assert.deepEqual(conn.layers, ["F.Cu", "F.Mask"]);
  assert.deepEqual(conn.kicad, { connector: true });
  assert.equal(padDlgTypeOf(conn), "conn");
  const again = padTypeSelected(conn, "smd", smd);
  assert.equal(again.kicad, undefined);
  assert.equal(padDlgTypeOf(again), "smd");
});

test("PadTypeSelected: SMD Aperture is an SMD pad on F.Paste only, with no number", () => {
  const smd = pad({ kind: "smd", drill: null, layers: ["F.Cu", "F.Paste", "F.Mask"] });
  const ap = padTypeSelected(smd, "aperture", smd);
  assert.equal(ap.kind, "smd");
  assert.deepEqual(ap.layers, ["F.Paste"]);
  assert.equal(ap.number, "");
  assert.equal(padDlgTypeOf(ap), "aperture");
});

test("PadTypeSelected: a hole type on a pad with no hole takes the current pad's (none for an SMD pad), the default pad 0.8 mm", () => {
  const smd = pad({ kind: "smd", drill: null, layers: ["F.Cu", "F.Paste", "F.Mask"] });
  assert.equal(padTypeSelected(smd, "pth", smd).drill, null, "an SMD pad's drill size is 0: padValuesOK then says the hole is missing");
  assert.equal(padTypeSelected(smd, "pth", null).drill, 800_000, "DEFAULT_PAD_DRILL_DIAMETER_MM for the default pad");
  const slotted = pad({ drill: null, drill_slot: [600_000, 1_200_000] });
  const viaSmd = padTypeSelected(padTypeSelected(slotted, "smd", slotted), "pth", slotted);
  assert.deepEqual([viaSmd.drill, viaSmd.drill_slot], [null, [600_000, 1_200_000]]);
});

test("padDlgTypeOf: the file's types read back", () => {
  assert.equal(padDlgTypeOf(pad()), "pth");
  assert.equal(padDlgTypeOf(pad({ kind: "non_plated_hole" })), "npth");
  assert.equal(padDlgTypeOf(pad({ kind: "smd", layers: [] })), "smd", "layers not set yet are the writer's SMD defaults");
  assert.equal(padDlgTypeOf(pad({ kind: "smd", layers: ["F.Paste"] })), "aperture");
});

test("PAD::SetAttribute: SMD to through-hole plumps the copper up to every layer; to SMD trims to one (B.Cu when it was on it)", () => {
  const smd = pad({ kind: "smd", drill: null, layers: ["F.Cu", "F.Paste", "F.Mask"] });
  const th = setPadAttribute(smd, "pth");
  assert.deepEqual(th.layers, ["*.Cu", "F.Paste", "F.Mask"]);
  assert.equal(th.drill, 800_000);
  const back = setPadAttribute(pad(), "smd");
  assert.deepEqual(back.layers, ["B.Cu", "F.Mask", "B.Mask"]);
  assert.equal(back.drill, null);
  assert.deepEqual(setPadAttribute(pad({ layers: ["F.Cu", "In1.Cu", "F.Mask"] }), "smd").layers, ["F.Cu", "F.Mask"]);
});

test("PAD::SetAttribute: NPTH clears the number; Connector sets the flag; the same type changes nothing", () => {
  const npth = setPadAttribute(pad({ number: "3" }), "npth");
  assert.equal(npth.number, "");
  assert.equal(npth.kind, "non_plated_hole");
  const conn = setPadAttribute(pad({ kind: "smd", drill: null, layers: ["F.Cu", "F.Mask"] }), "conn");
  assert.deepEqual(conn.kicad, { connector: true });
  const p = pad();
  assert.equal(setPadAttribute(p, "pth"), p);
});
