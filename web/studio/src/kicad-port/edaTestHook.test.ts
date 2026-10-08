import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ErrorLog,
  boardLists,
  countsOf,
  describeActions,
  disabledReason,
  footprintLists,
  formatArgs,
  isNoise,
  isTrackedUrl,
  kindIndex,
  newNames,
  openFlags,
  runOutcome,
  schematicLists,
  selectionWithKinds,
  settle,
  symbolLists,
} from "./edaTestHook";

test("passive-listener noise is not an error; anything else is", () => {
  assert.equal(isNoise("Unable to preventDefault inside passive event listener invocation."), true);
  assert.equal(isNoise("TypeError: x is undefined"), false);
});

test("the error log keeps errors in order with their time, drops noise and blanks, and filters by time", () => {
  let now = 1000;
  const log = new ErrorLog(10, () => now);
  log.add("first");
  now = 2000;
  log.add("Unable to preventDefault inside passive event listener invocation.");
  log.add("   ");
  log.add("second");
  assert.deepEqual(log.list(), [
    { time: 1000, message: "first" },
    { time: 2000, message: "second" },
  ]);
  assert.deepEqual(log.list(1500), [{ time: 2000, message: "second" }]);
});

test("the error log is bounded: the oldest go first", () => {
  const log = new ErrorLog(3, () => 1);
  for (const m of ["a", "b", "c", "d"]) log.add(m);
  assert.deepEqual(log.list().map((e) => e.message), ["b", "c", "d"]);
});

test("console.error arguments become one line", () => {
  assert.equal(formatArgs(["failed:", new Error("boom"), { a: 1 }, 7]), 'failed: boom {"a":1} 7');
});

test("an action is disabled with a reason: no handler, or an action of another editor", () => {
  assert.equal(disabledReason("common.Control.zoomIn", "pcb", true), null);
  assert.match(disabledReason("common.Control.zoomIn", "pcb", false)!, /no handler/);
  assert.match(disabledReason("pcbnew.InteractiveRouter.SingleTrack", "schematic", true)!, /not offered on the schematic tab/);
  assert.equal(disabledReason("pcbnew.InteractiveRouter.SingleTrack", "pcb", true), null);
});

test("the action list is sorted, labelled and says why a disabled one is; unhandled ones come only when asked for", () => {
  const labels = new Map([["common.Control.zoomIn", "Zoom In"]]);
  const list = describeActions(["pcbnew.Zzz.last", "common.Control.zoomIn", "eeschema.Aaa.first"], "pcb", labels);
  assert.deepEqual(list.map((a) => a.id), ["common.Control.zoomIn", "eeschema.Aaa.first", "pcbnew.Zzz.last"]);
  assert.deepEqual(list[0], { id: "common.Control.zoomIn", label: "Zoom In", enabled: true });
  assert.equal(list[1]!.enabled, false);
  assert.match(list[1]!.reason!, /another editor/);
  assert.equal(list[2]!.enabled, true);
  const all = describeActions(["common.Control.zoomIn"], "pcb", labels, ["common.Control.unwired"]);
  assert.deepEqual(all.find((a) => a.id === "common.Control.unwired"), { id: "common.Control.unwired", label: null, enabled: false, reason: disabledReason("x", "pcb", false)! });
});

test("the selection comes with the kind of each item, sorted by id", () => {
  const index = kindIndex({ track: ["t1", "t2"], via: ["v1"], footprint: ["U1"] });
  assert.deepEqual(selectionWithKinds(new Set(["v1", "U1", "t2", "ghost"]), index), [
    { id: "U1", kind: "footprint" },
    { id: "ghost", kind: "unknown" },
    { id: "t2", kind: "track" },
    { id: "v1", kind: "via" },
  ]);
});

test("an id on two lists has the kind of the first", () => {
  assert.equal(kindIndex({ a: ["x"], b: ["x"] }).get("x"), "a");
});

test("the board and the schematic lists carry every kind the editors select", () => {
  const b = boardLists({ parts: [{ ref: "U1", pads: [{ num: "1" }, { num: "1" }] }], routing: { tracks: [{ id: "t" }], vias: [{ id: "v" }], zones: [{ id: "z" }] }, drawings: { shapes: [{ id: "s" }], texts: [{ id: "x" }], dimensions: [{ id: "d" }], groups: [{ id: "g" }] } });
  assert.deepEqual(b, { footprint: ["U1"], pad: ["U1.1", "U1.1#2"], track: ["t"], via: ["v"], zone: ["z"], shape: ["s"], text: ["x"], dimension: ["d"], group: ["g"] });
  assert.deepEqual(Object.values(boardLists(null)).flat(), []);
  const s = schematicLists({ symbols: [{ id: "R1" }], wires: [{ id: "w" }], labels: [{ id: "l" }] });
  assert.deepEqual([s.symbol, s.wire, s.label], [["R1"], ["w"], ["l"]]);
  assert.deepEqual(Object.values(schematicLists(undefined)).flat(), []);
});

test("the library editors' lists leave out items that have no id", () => {
  assert.deepEqual(footprintLists({ pads: [{ id: "p1" }, {}], graphics: [{ id: "g1" }], texts: [{ id: "t1" }] }), { pad: ["p1"], graphic: ["g1"], text: ["t1"] });
  assert.deepEqual(symbolLists({ pins: [{ id: "n1" }, { id: undefined }], graphics: [{ id: "g1" }] }), { pin: ["n1"], graphic: ["g1"] });
  assert.deepEqual(Object.values(footprintLists(null)).flat(), []);
  assert.deepEqual(Object.values(symbolLists(undefined)).flat(), []);
});

test("counts are the sizes of the lists, zero when there is no board or schematic", () => {
  assert.deepEqual(countsOf(null, null), { footprints: 0, tracks: 0, vias: 0, zones: 0, symbols: 0, wires: 0, labels: 0 });
  assert.deepEqual(
    countsOf({ parts: [{ ref: "a" }, { ref: "b" }], routing: { tracks: [{ id: "1" }], vias: [], zones: [{ id: "z" }] } }, { symbols: [{ id: "R1" }], wires: [{ id: "1" }, { id: "2" }], labels: [] }),
    { footprints: 2, tracks: 1, vias: 0, zones: 1, symbols: 1, wires: 2, labels: 0 }
  );
});

test("the open flags name every dialog or panel a store has switched on", () => {
  assert.deepEqual(openFlags({ drcOpen: true, plotDialogOpen: false, schDialog: "find", textDialog: { mode: "add" }, other: true, nothing: null, emptyDialog: "", tab: "pcb" }), ["drcOpen", "schDialog:find", "textDialog"]);
});

test("the dialogs an action opened are the names that were not there before", () => {
  assert.deepEqual(newNames(["About"], ["About", "Page Settings"]), ["Page Settings"]);
  assert.deepEqual(newNames([], []), []);
});

test("a run is ok unless it threw, a command was refused or an error was logged", () => {
  assert.deepEqual(runOutcome(undefined, [], []), { ok: true });
  assert.deepEqual(runOutcome(new Error("boom"), [], []), { ok: false, error: "boom" });
  assert.deepEqual(runOutcome(undefined, [{ ok: true }, { ok: false, message: "refused: nothing to undo" }], []), { ok: false, error: "refused: nothing to undo" });
  assert.deepEqual(runOutcome(undefined, [{ ok: true }], [{ time: 1, message: "Item locked." }]), { ok: false, error: "Item locked." });
  assert.deepEqual(runOutcome(undefined, [{ ok: false }], []), { ok: false, error: "the command was refused" });
});

test("only the studio's own API calls are waited for, not its polls", () => {
  assert.equal(isTrackedUrl("/api/cmd"), true);
  assert.equal(isTrackedUrl("http://127.0.0.1:8792/api/state"), true);
  assert.equal(isTrackedUrl("/api/version"), false);
  assert.equal(isTrackedUrl("/api/view?x=1"), false);
  assert.equal(isTrackedUrl("/assets/index.js"), false);
});

test("settle returns once nothing has been in flight for the quiet period", async () => {
  let t = 0;
  const pendingAt = (time: number) => (time < 100 ? 1 : 0);
  const deps = { pending: () => pendingAt(t), delay: async (ms: number) => void (t += ms), now: () => t, quietMs: 60, timeoutMs: 1000, stepMs: 20 };
  assert.equal(await settle(deps), "settled");
  assert.ok(t >= 160, `waited for the request (100) and the quiet period (60): ${t}`);
});

test("settle gives up when requests never stop", async () => {
  let t = 0;
  const result = await settle({ pending: () => 1, delay: async (ms) => void (t += ms), now: () => t, timeoutMs: 200, stepMs: 20 });
  assert.equal(result, "timeout");
});
