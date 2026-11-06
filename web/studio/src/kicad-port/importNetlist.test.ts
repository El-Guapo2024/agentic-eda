import { test } from "node:test";
import assert from "node:assert/strict";
import {
  BUTTON_CLOSE,
  BUTTON_TEST,
  BUTTON_UPDATE,
  DEFAULT_NETLIST_OPTIONS,
  DIALOG_TITLE,
  headLines,
  importNetlistCmd,
  LABEL_APPLIED,
  LABEL_TO_APPLY,
  LINK_CHOICES,
  OPTION_LABELS,
  SHOW_ALL,
  sheetOfPath,
  shownLines,
  spreadBySheet,
  updateOutcome,
  type NetlistReportLine,
  type SpreadItem,
} from "./importNetlist";

test("the dialog's words are DIALOG_IMPORT_NETLIST_BASE's and loadNetlist's", () => {
  assert.equal(DIALOG_TITLE, "Import Netlist");
  assert.deepEqual([...LINK_CHOICES], ["Link footprints using component tstamps (unique ids)", "Link footprints using reference designators"]);
  assert.deepEqual(
    OPTION_LABELS.map(([, t]) => t),
    ["Delete footprints with no components in netlist", "Replace footprints with those specified in netlist", "Group footprints based on symbol group", "Delete/replace footprints even if locked", "Delete tracks shorting multiple nets"]
  );
  assert.deepEqual([BUTTON_TEST, BUTTON_UPDATE, BUTTON_CLOSE], ["Load and Test Netlist", "Update PCB", "Close"]);
  assert.deepEqual([LABEL_TO_APPLY, LABEL_APPLIED], ["Changes to Be Applied", "Changes Applied to PCB"]);
  assert.deepEqual(headLines("a.net", true).map((l) => l.text), ["Reading netlist file 'a.net'.", "Using tstamps (unique IDs) to match symbols and footprints."]);
  assert.equal(headLines("a.net", false)[1]!.text, "Using reference designators to match symbols and footprints.");
});

test("the choices start as the dialog's: tstamps, replace footprints and group footprints on, the rest off", () => {
  assert.deepEqual(DEFAULT_NETLIST_OPTIONS, { by_timestamp: true, delete_extra_footprints: false, replace_footprints: true, transfer_groups: true, override_locks: false, delete_shorting_tracks: false });
});

test("Update PCB is one import_netlist command with the file and the choices", () => {
  const cmd = importNetlistCmd({ name: "a.net", text: "(export)" }, { ...DEFAULT_NETLIST_OPTIONS, delete_extra_footprints: true });
  assert.deepEqual(cmd, { op: "import_netlist", text: "(export)", name: "a.net", options: { ...DEFAULT_NETLIST_OPTIONS, delete_extra_footprints: true } });
  assert.deepEqual(importNetlistCmd({ name: "a.net", text: "" }, DEFAULT_NETLIST_OPTIONS, { x: 1, y: 2 }), { op: "import_netlist", text: "", name: "a.net", options: DEFAULT_NETLIST_OPTIONS, at: { x: 1, y: 2 } });
});

test("the Show: boxes let through the severities they name", () => {
  const lines: NetlistReportLine[] = [
    { severity: "info", text: "i" },
    { severity: "action", text: "a" },
    { severity: "warning", text: "w" },
    { severity: "error", text: "e" },
  ];
  assert.equal(shownLines(lines, SHOW_ALL).length, 4);
  assert.deepEqual(shownLines(lines, { ...SHOW_ALL, info: false, action: false }).map((l) => l.text), ["w", "e"]);
});

test("Update PCB's reply: added footprints wait for Close, none ends the step at once, a refusal is the last line", () => {
  const file = { name: "a.net", text: "" };
  const report = { ok: true, lines: [{ severity: "action" as const, text: "Added R1 (footprint 'R:R')." }], added: ["R1"], added_paths: ["/x/y"], warnings: 0, errors: 0 };
  assert.deepEqual(updateOutcome({ ok: true, message: "", report }, file, true), { lines: report.lines, alert: null, commitNow: false, carry: { refs: ["R1"], paths: ["/x/y"] } });
  const none = updateOutcome({ ok: true, message: "", report: { ...report, added: [], added_paths: [] } }, file, true);
  assert.equal(none.commitNow, true);
  assert.equal(none.carry, null);
  const refused = updateOutcome({ ok: false, message: "that move takes the board from 0 failure(s) to 1 (x); refused because --strict" }, file, false);
  assert.equal(refused.lines.length, 3);
  assert.equal(refused.lines[2]!.severity, "error");
  assert.match(refused.lines[2]!.text, /refused because --strict/);
  const unreadable = updateOutcome({ ok: false, message: "Cannot open netlist file 'a.net'.", report: { ok: false, lines: headLines("a.net", true), added: [], warnings: 0, errors: 0, error: "Cannot open netlist file 'a.net'." } }, file, true);
  assert.equal(unreadable.alert, "Cannot open netlist file 'a.net'.");
});

test("SpreadFootprints groups by sheet: a footprint's sheet is its path without the last step", () => {
  assert.equal(sheetOfPath("/aaa/bbb"), "/aaa");
  assert.equal(sheetOfPath("/bbb"), "");
  assert.equal(sheetOfPath(""), "");
});

test("SpreadFootprints of footprints piled on one point: no two overlap, each sheet's block keeps together, the arrangement starts at their top-left", () => {
  const box = (w: number): readonly [number, number, number, number] => [-w, -1_000_000, w, 1_000_000];
  const items: SpreadItem[] = [
    { ref: "R1", bbox: box(1_000_000), path: "/s1/a" },
    { ref: "R2", bbox: box(1_000_000), path: "/s1/b" },
    { ref: "R3", bbox: box(1_000_000), path: "/s2/c" },
    { ref: "J1", bbox: box(3_000_000), path: "/s2/d" },
  ];
  const off = spreadBySheet(items);
  const moved = items.map((i) => {
    const [dx, dy] = off[i.ref]!;
    return { ref: i.ref, b: [i.bbox[0] + dx, i.bbox[1] + dy, i.bbox[2] + dx, i.bbox[3] + dy] as const };
  });
  for (let i = 0; i < moved.length; i++)
    for (let j = i + 1; j < moved.length; j++) {
      const a = moved[i]!.b;
      const b = moved[j]!.b;
      const overlap = a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
      assert.equal(overlap, false, `${moved[i]!.ref} and ${moved[j]!.ref} overlap`);
    }
  assert.equal(Math.min(...moved.map((m) => m.b[0])), -3_000_000);
  assert.equal(Math.min(...moved.map((m) => m.b[1])), -1_000_000);
  // one sheet: Pack and Move's own arrangement
  const one = spreadBySheet(items.slice(0, 2));
  assert.equal(Object.keys(one).length, 2);
  assert.deepEqual(spreadBySheet([]), {});
});
