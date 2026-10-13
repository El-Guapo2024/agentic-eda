import { test } from "node:test";
import assert from "node:assert/strict";
import type { FpCheckMarker } from "../api/types";
import { badgeText, badges, markerItems, markerLayer, setShowAll, shown, showAll, SHOW_ALL, visibleMarkers } from "./footprintChecker";

const marker = (type: string, severity: FpCheckMarker["severity"], ids: (string | null)[] = []): FpCheckMarker => ({ type, description: type, severity, at: [0, 0], items: ids.map((id) => ({ description: "d", pos: [0, 0], id })) });
const list = [marker("missing_courtyard", "warning"), marker("shorting_items", "error", ["p1", "p2"]), marker("malformed_courtyard", "error", ["g1", null, "g1"]), marker("holes_co_located", "warning", ["p3"])];

test("the Show boxes let errors, warnings and exclusions through", () => {
  assert.deepEqual(visibleMarkers(list, SHOW_ALL), [0, 1, 2, 3]);
  assert.deepEqual(visibleMarkers(list, { errors: true, warnings: false, exclusions: true }), [1, 2]);
  assert.deepEqual(visibleMarkers(list, { errors: false, warnings: true, exclusions: false }), [0, 3]);
  assert.ok(!shown(marker("x", "ignore"), SHOW_ALL), "an ignored check is never a marker");
});

test("the All box: on turns the three on, off leaves the errors alone", () => {
  assert.ok(showAll(SHOW_ALL) && !showAll({ errors: true, warnings: false, exclusions: true }));
  assert.deepEqual(setShowAll(true), SHOW_ALL);
  assert.deepEqual(setShowAll(false), { errors: true, warnings: false, exclusions: false });
});

test("the badges say '-' until the checks have run, a count after", () => {
  assert.deepEqual(badges([], false), { errors: null, warnings: null, exclusions: 0 });
  assert.equal(badgeText(badges([], false).errors), "-");
  assert.deepEqual(badges([], true), { errors: 0, warnings: 0, exclusions: 0 });
  assert.deepEqual(badges(list, true), { errors: 2, warnings: 2, exclusions: 0 });
  // markers left from a run keep their counts when the flag is not set (a footprint reopened)
  assert.deepEqual(badges(list, false), { errors: 2, warnings: 2, exclusions: 0 });
});

test("a courtyard marker brings the courtyard layer to the front, a silk one the silkscreen", () => {
  assert.equal(markerLayer(list[0]!), "F.CrtYd");
  assert.equal(markerLayer(list[2]!), "F.CrtYd");
  assert.equal(markerLayer(marker("silk_over_copper", "error")), "F.SilkS");
  assert.equal(markerLayer(list[1]!), null);
});

test("the items of a marker are the editor's own, once each", () => {
  assert.deepEqual(markerItems(list[1]!), ["p1", "p2"]);
  assert.deepEqual(markerItems(list[2]!), ["g1"]);
  assert.deepEqual(markerItems(list[0]!), []);
});
