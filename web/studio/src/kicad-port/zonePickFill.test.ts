import { test } from "node:test";
import assert from "node:assert/strict";
import { setZonePickFills, zoneFillHit } from "./zonePickFill";
import type { FillReport } from "../api/types";

const z = { id: "z", layer: "F.Cu", layers: ["F.Cu"] };
const fill: FillReport = { source: null, zones: [{ id: "z", net: "GND", layer: "F.Cu", filled: true, area_nm2: 1, fragments: [[[0, 0], [10, 0], [10, 10], [0, 10]]] }] };

test("PP10: a click inside a pour hits it where it is filled on a visible layer, only while fills are shown filled; an unfilled pour never", () => {
  setZonePickFills(null, "filled");
  assert.equal(zoneFillHit(z, 5, 5, {}), null, "no fills handed over: the caller takes the zone's inside");
  setZonePickFills(fill, "filled");
  assert.equal(zoneFillHit(z, 5, 5, {}), true);
  assert.equal(zoneFillHit(z, 15, 5, {}), false, "outside the fill (a clearance gap, a removed island)");
  assert.equal(zoneFillHit(z, 5, 5, { "F.Cu": false }), false, "the layer is hidden");
  assert.equal(zoneFillHit({ id: "other", layer: "F.Cu" }, 5, 5, {}), false, "an unfilled pour: its outline only");
  setZonePickFills(fill, "outline");
  assert.equal(zoneFillHit(z, 5, 5, {}), false, "`SetIgnoreZoneFills` unless SHOW_FILLED");
  setZonePickFills(null, "filled");
});
