import { test } from "node:test";
import assert from "node:assert/strict";
import { footprintZoneForm, NO_PAD_ZONE_FACTS, padNumbers, padZoneForm, padZoneIsSet, setFootprintZoneCmd, setPadZoneCmd, zoneCmds } from "./padZone";
import type { Part } from "../api/types";

function part(over: Partial<Part> = {}): Part {
  return {
    ref: "U1",
    value: null,
    package: null,
    mpn: null,
    footprint: "SOIC-8",
    block: null,
    placed: true,
    size: null,
    pads: [
      { num: "1", net: "GND", x: 0, y: 0, w: 600, h: 600, round: false, th: false },
      { num: "2", net: null, x: 1000, y: 0, w: 600, h: 600, round: false, th: false },
      { num: "2", net: null, x: 2000, y: 0, w: 600, h: 600, round: false, th: false },
      { num: "", net: null, x: 3000, y: 0, w: 600, h: 600, round: true, th: true },
    ],
    ...over,
  };
}

test("a part's pad numbers are each listed once, in the order met, and a numberless pad is left out", () => {
  assert.deepEqual(padNumbers(part()), ["1", "2"]);
  assert.deepEqual(padNumbers(undefined), []);
});

test("a pad that sets nothing inherits everything", () => {
  const f = padZoneForm(part(), "1");
  assert.deepEqual(f, NO_PAD_ZONE_FACTS);
  assert.equal(padZoneIsSet(f), false);
  assert.deepEqual(footprintZoneForm(part()), { connection: null, clearance: null });
});

test("the form shows what the part carries for a pad and for its footprint", () => {
  const p = part({
    zone: { connection: "Full", clearance: 800, pads: [{ num: "1", connection: "Thermal", gap: 400, spoke_width: 300, spoke_angle_mdeg: 45_000, clearance: 600 }] },
  });
  assert.deepEqual(padZoneForm(p, "1"), { connection: "Thermal", gap: 400, spokeWidth: 300, spokeAngleMdeg: 45_000, clearance: 600 });
  assert.deepEqual(padZoneForm(p, "2"), NO_PAD_ZONE_FACTS, "another pad of the footprint has its own, empty");
  assert.deepEqual(footprintZoneForm(p), { connection: "Full", clearance: 800 });
  assert.equal(padZoneIsSet(padZoneForm(p, "1")), true);
});

test("setPadZoneCmd is a whole-panel commit: null inherits, lengths are whole and not negative, an angle stays within one turn", () => {
  const c = setPadZoneCmd("U1", "1", { connection: "Full", gap: 400.4, spokeWidth: -5, spokeAngleMdeg: 405_000, clearance: null });
  assert.deepEqual(c, { op: "set_pad_zone_overrides", part: "U1", pad: "1", zone_connection: "Full", thermal_gap: 400, thermal_spoke_width: 0, thermal_spoke_angle_mdeg: 45_000, clearance: null });
  assert.deepEqual(setPadZoneCmd("U1", "2", NO_PAD_ZONE_FACTS), { op: "set_pad_zone_overrides", part: "U1", pad: "2", zone_connection: null, thermal_gap: null, thermal_spoke_width: null, thermal_spoke_angle_mdeg: null, clearance: null });
  const neg = setPadZoneCmd("U1", "1", { ...NO_PAD_ZONE_FACTS, spokeAngleMdeg: -45_000 });
  assert.equal(neg.op === "set_pad_zone_overrides" && neg.thermal_spoke_angle_mdeg, 315_000);
});

test("setFootprintZoneCmd sends the footprint's connection and clearance", () => {
  assert.deepEqual(setFootprintZoneCmd("U1", { connection: "None", clearance: 900 }), { op: "set_footprint_zone_connection", part: "U1", zone_connection: "None", clearance: 900 });
  assert.deepEqual(setFootprintZoneCmd("U1", { connection: null, clearance: null }), { op: "set_footprint_zone_connection", part: "U1", zone_connection: null, clearance: null });
});

test("a dialog's OK sends the zone commands of what it changed and nothing for what it left, the footprint first", () => {
  const p = part({ zone: { connection: "Thermal", clearance: 200, pads: [{ num: "1", connection: "Full", gap: 300, spoke_width: null, spoke_angle_mdeg: null, clearance: null }] } });
  const same = footprintZoneForm(p);
  assert.deepEqual(zoneCmds(p, same, {}), [], "no form was opened");
  assert.deepEqual(zoneCmds(p, same, { "1": padZoneForm(p, "1"), "2": padZoneForm(p, "2") }), [], "forms that were opened and left as they were");
  const cmds = zoneCmds(p, { connection: null, clearance: 200 }, { "1": { ...padZoneForm(p, "1"), clearance: 250 }, "2": { ...NO_PAD_ZONE_FACTS, connection: "None" } });
  assert.deepEqual(
    cmds.map((c) => (c.op === "set_footprint_zone_connection" ? [c.op, c.zone_connection, c.clearance] : c.op === "set_pad_zone_overrides" ? [c.op, c.pad, c.zone_connection, c.clearance] : c.op)),
    [["set_footprint_zone_connection", null, 200], ["set_pad_zone_overrides", "1", "Full", 250], ["set_pad_zone_overrides", "2", "None", null]],
    "the footprint's, then each pad number that changed, a pad's whole panel at a time"
  );
});

