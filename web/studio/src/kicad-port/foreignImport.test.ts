import { test } from "node:test";
import assert from "node:assert/strict";
import {
  BOARD_ACCEPT,
  SCHEMATIC_ACCEPT,
  acceptFor,
  addMappings,
  asForeignParam,
  autoMatch,
  describeImport,
  describeIssues,
  initialMapping,
  isRequiredLayer,
  issuesOf,
  keptGuesses,
  kicadLayerLabel,
  kicadLayerList,
  layerMapRequest,
  removeMappings,
  unmatchedRequired,
  wrapRequired,
  type ForeignReply,
  type SourceLayer,
} from "./foreignImport";

/** What kicad-cli's report says of an Eagle board: the layers it has a guess for, and the ones it has not (`UNDEFINED`). */
const LAYERS: SourceLayer[] = [
  { source: "Top", kicad: "F.Cu", explicit: false },
  { source: "Bottom", kicad: "B.Cu", explicit: false },
  { source: "Route2", kicad: "In1.Cu", explicit: false },
  { source: "tNames", kicad: "F.SilkS", explicit: false },
  { source: "bNames", kicad: "B.SilkS", explicit: false },
  { source: "200bmp", kicad: null, explicit: false },
  { source: "Unrouted", kicad: null, explicit: false },
];

test("the file chooser offers the formats kicad-cli's import takes", () => {
  assert.equal(acceptFor("board"), BOARD_ACCEPT);
  assert.equal(acceptFor("schematic"), SCHEMATIC_ACCEPT);
  for (const ext of [".brd", ".PcbDoc", ".cpa", ".asc", ".json", ".zip"]) assert.ok(BOARD_ACCEPT.split(",").includes(ext), ext);
  for (const ext of [".sch", ".SchDoc", ".csa", ".json"]) assert.ok(SCHEMATIC_ACCEPT.split(",").includes(ext), ext);
  assert.ok(!SCHEMATIC_ACCEPT.split(",").includes(".brd") && !BOARD_ACCEPT.split(",").includes(".SchDoc"));
});

test("the action takes the file the page read, with the answers a dialog would give", () => {
  assert.deepEqual(asForeignParam({ name: "a.brd", data: "AAAA", skipLayerMapping: true }), { name: "a.brd", data: "AAAA", skipLayerMapping: true });
  assert.equal(asForeignParam({ name: "a.brd" }), null);
  assert.equal(asForeignParam({ data: "AAAA" }), null);
  assert.equal(asForeignParam("a.brd"), null);
  assert.equal(asForeignParam(null), null);
});

test("what an import brought is said with the right plural, and what kicad-cli reported is counted by messages said", () => {
  const board: ForeignReply = { ok: true, kind: "board", name: "x.brd", format: "Eagle", counts: { footprints: 19, nets: 1, tracks: 82, vias: 0, zones: 2 } };
  assert.equal(describeImport(board), "Imported x.brd (Eagle): 19 footprints, 1 net, 82 tracks, 2 zones.");
  const sch: ForeignReply = { ok: true, kind: "schematic", name: "p.SchDoc", format: "Altium", counts: { symbols: 49, sheets: 1, wires: 0, labels: 12 } };
  assert.equal(describeImport(sch), "Imported p.SchDoc (Altium): 49 symbols, 1 sheet, 12 labels.");
  assert.equal(describeImport({ ok: true, kind: "board", counts: {} }), "Imported the file: nothing.");

  const findings = [
    { severity: "error" as const, message: "No library symbol found for 'R1'", count: 49 },
    { severity: "warning" as const, message: "Font not found", count: 1 },
    { severity: "info" as const, message: "Setting symbol", count: 36 },
  ];
  assert.equal(describeIssues(findings), "49 errors and 1 warning were reported.");
  assert.equal(describeIssues([{ severity: "warning", message: "w", count: 1 }]), "1 warning was reported.");
  assert.equal(describeIssues([]), "");
  assert.equal(describeIssues(undefined), "");
  assert.deepEqual(issuesOf(findings).map((f) => f.severity), ["error", "warning"], "an information line is not an issue");
});

test("the KiCad Layers list is LSET::UIOrder: the copper the board has, then the technical and user layers", () => {
  const list = kicadLayerList(LAYERS);
  assert.deepEqual(list.slice(0, 3), ["F.Cu", "In1.Cu", "B.Cu"], "copper from the front to the back; only the inner layers the importer used");
  assert.deepEqual(list.slice(3, 9), ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS"]);
  assert.ok(list.indexOf("Edge.Cuts") < list.indexOf("Margin") && list.indexOf("Margin") < list.indexOf("F.CrtYd") && list.indexOf("B.Fab") < list.indexOf("User.1"));
  assert.equal(list[list.length - 1], "User.9");
  assert.equal(new Set(list).size, list.length);
  // Two layers are always there, even when nothing was mapped to them.
  assert.deepEqual(kicadLayerList([]).slice(0, 2), ["F.Cu", "B.Cu"]);
  // The names the dialog shows are the ones KiCad shows (`LayerName`).
  assert.equal(kicadLayerLabel("F.SilkS"), "F.Silkscreen");
  assert.equal(kicadLayerLabel("Dwgs.User"), "User.Drawings");
  assert.equal(kicadLayerLabel("In1.Cu"), "In1.Cu");
});

test("the dialog opens as KiCad's does after Auto-Match Layers: the importer's guesses are matched, the layers it has no guess for are not", () => {
  const s = initialMapping(LAYERS);
  assert.deepEqual(s.unmatched, ["200bmp", "Unrouted"]);
  assert.deepEqual(s.matched.map((m) => `${m.source}>${m.kicad}`), ["Top>F.Cu", "Bottom>B.Cu", "Route2>In1.Cu", "tNames>F.SilkS", "bNames>B.SilkS"]);
  assert.deepEqual(layerMapRequest(s), {}, "nothing the person changed: nothing to send, and the first run's board is the board");
  // A layer that goes to copper is marked * and must stay matched.
  assert.deepEqual(LAYERS.filter(isRequiredLayer).map((l) => l.source), ["Top", "Bottom", "Route2"]);
  assert.equal(wrapRequired("Top", true), "Top *");
  assert.equal(wrapRequired("tNames", false), "tNames");
  assert.deepEqual(unmatchedRequired(s, LAYERS), []);
});

test("> moves unmatched layers to the KiCad layer chosen; the pairs the person made are what is sent", () => {
  let s = initialMapping(LAYERS);
  s = addMappings(s, ["200bmp", "Unrouted"], "Dwgs.User");
  assert.deepEqual(s.unmatched, []);
  assert.deepEqual(layerMapRequest(s), { "200bmp": "Dwgs.User", Unrouted: "Dwgs.User" });
  // A matched layer is not an unmatched one: > does nothing to it (the pair is removed and added again to move it).
  assert.equal(addMappings(s, ["tNames"], "F.Fab"), s);
  // Nothing chosen: nothing happens.
  const untouched = initialMapping(LAYERS);
  assert.equal(addMappings(untouched, ["200bmp"], ""), untouched);
  // Moving a layer the importer had a guess for: < then > to another layer; that pair differs from the guess and is sent.
  s = removeMappings(s, ["tNames"]);
  assert.ok(s.unmatched.includes("tNames"));
  assert.deepEqual(layerMapRequest(s), { "200bmp": "Dwgs.User", Unrouted: "Dwgs.User" }, "unmatched: no pair");
  s = addMappings(s, ["tNames"], "F.Fab");
  assert.equal(layerMapRequest(s).tNames, "F.Fab");
  // Put back on its guess: no pair to send for it.
  s = removeMappings(s, ["tNames"]);
  s = addMappings(s, ["tNames"], "F.SilkS");
  assert.equal(layerMapRequest(s).tNames, undefined);
});

test("< and << put layers back, Auto-Match matches what the importer has a guess for, and a required layer must be matched", () => {
  let s = initialMapping(LAYERS);
  s = removeMappings(s, "all");
  assert.equal(s.matched.length, 0);
  assert.equal(s.unmatched.length, LAYERS.length);
  assert.deepEqual(unmatchedRequired(s, LAYERS).sort(), ["Bottom", "Route2", "Top"], "OK would be refused: All required layers (marked with '*') must be matched");
  // The ones with no guess are not matched by Auto-Match.
  s = autoMatch(s);
  assert.deepEqual(s.unmatched.sort(), ["200bmp", "Unrouted"]);
  assert.equal(s.matched.length, 5);
  assert.deepEqual(unmatchedRequired(s, LAYERS), []);
  assert.deepEqual(layerMapRequest(s), {});
  // A layer left out that has a guess keeps it in kicad-cli: the dialog says which.
  s = removeMappings(s, ["tNames", "bNames"]);
  assert.deepEqual(keptGuesses(s).sort(), ["bNames", "tNames"]);
  assert.deepEqual(layerMapRequest(s), {}, "and no pair is sent for them");
  // Nothing to remove, nothing to add: the same state.
  assert.equal(removeMappings(s, ["nothing"]), s);
  const empty = initialMapping([]);
  assert.equal(autoMatch(empty), empty);
});
