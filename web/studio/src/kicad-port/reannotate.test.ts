import { test } from "node:test";
import assert from "node:assert/strict";
import type { ReannotatePlan } from "../api/fpAssign";
import { DIRECTIONS, badLines, confirmQuestion, defaultForm, doneLines, filterPrefix, formOptions, planLines, renamedSelection, startNumber } from "./reannotate";

const mm = (um: number) => `${(um / 1000).toFixed(1)} mm`;

const plan = (extra: Partial<ReannotatePlan> = {}): ReannotatePlan => ({
  front: [
    { index: 1, reference: "R2", x: 10_000, y: 20_000, rounded_x: 10_160, rounded_y: 20_320 },
    { index: 2, reference: "R1", x: 30_000, y: 20_000, rounded_x: 30_480, rounded_y: 20_320 },
  ],
  back: [],
  prefixes: ["R", "C"],
  excluded: [],
  changes: [
    { old: "R1", new: "R2", front: true, action: "update" },
    { old: "R2", new: "R1", front: true, action: "update" },
  ],
  bad: [],
  errors: [],
  ...extra,
});

test("the dialog opens with the first order, the footprint's location, the editor's grid and the whole board; a selection makes the scope the selection", () => {
  const f = defaultForm(1270, false);
  assert.deepEqual([f.direction, f.location, f.gridUm, f.scope, f.frontStart, f.backStart, f.frontPrefix, f.excludeLocked], [0, "footprint", 1270, "all", "1", "", "", false]);
  assert.equal(defaultForm(1270, true).scope, "selection");
  assert.equal(DIRECTIONS.length, 8);
});

test("a start field is a whole number as wxAtoi reads it: blank is zero", () => {
  assert.deepEqual(["1", "", "  12", "007", "abc", "5x", "-3"].map(startNumber), [1, 0, 12, 7, 0, 5, 0]);
});

test("a prefix keeps letters, digits and _-+=/\\ and nothing else", () => {
  assert.equal(filterPrefix("F_"), "F_");
  assert.equal(filterPrefix("B-1 x!+="), "B-1x+=");
  assert.equal(filterPrefix("a/b\\c"), "a/b\\c");
});

test("the options are the controls with the numbers read and the prefixes filtered", () => {
  const f = { ...defaultForm(635, false), direction: 5, frontStart: "10", backStart: "100", frontPrefix: "F ", backPrefix: "B_", exclude: "R C7*", scope: "front" as const, removeBackPrefix: true };
  assert.deepEqual(formOptions(f), {
    direction: 5,
    location: "footprint",
    grid_um: 635,
    scope: "front",
    exclude_locked: false,
    exclude: "R C7*",
    front_start: 10,
    back_start: 100,
    front_prefix: "F",
    back_prefix: "B_",
    remove_front_prefix: false,
    remove_back_prefix: true,
  });
});

test("the message window tells the lists, the prefixes and the change log in KiCad's order and words", () => {
  const lines = planLines(plan(), "footprint", mm);
  assert.deepEqual(
    lines.map((l) => l.text),
    [
      "Front Footprints (sorted by footprint location)",
      "-------------------------------------------------------------",
      "1 R2 at 10.0 mm, 20.0 mm (rounded to 10.2 mm, 20.3 mm)",
      "2 R1 at 30.0 mm, 20.0 mm (rounded to 30.5 mm, 20.3 mm)",
      "",
      "Reference Designator Prefixes in Use",
      "-------------------------------------------------------------",
      "R C",
      "",
      "Change Log",
      "-------------------------------------------------------------",
      "R1 -> R2",
      "R2 -> R1",
    ],
  );
  assert.deepEqual(lines.slice(0, 5).map((l) => l.severity), ["info", "info", "info", "info", "info"]);
  assert.equal(lines.at(-1)!.severity, "action");
  assert.ok(planLines(plan(), "reference", mm)[0]!.text.endsWith("(sorted by reference designator location)"));
});

test("the back list, the exclusion list and the footprints that are not updated are told", () => {
  const p = plan({
    back: [{ index: 1, reference: "C3", x: 1000, y: 2000, rounded_x: 1000, rounded_y: 2000 }],
    excluded: ["R", "C7*"],
    changes: [
      { old: "", new: "", front: true, action: "empty" },
      { old: "C3", new: "C3", front: false, action: "exclude" },
      { old: "TP", new: "TP", front: true, action: "invalid" },
      { old: "U1", new: "U1", front: true, action: "update" },
    ],
    prefixes: Array.from({ length: 17 }, (_, i) => `P${i}`),
  });
  const text = planLines(p, "footprint", mm).map((l) => l.text);
  assert.ok(text.includes("Back Footprints (sorted by footprint location)"));
  assert.ok(text.includes("(Excluding R C7* from reannotation.)"));
  assert.ok(text.includes("unannotated footprint  (not updated)"));
  assert.ok(text.includes("C3  (excluded)"));
  assert.ok(text.includes("TP  (unannotated; not updated)"));
  assert.ok(text.includes("U1 -> U1"));
  assert.ok(text.includes("P16"), "sixteen prefixes to a line");
  assert.ok(text.includes(Array.from({ length: 16 }, (_, i) => `P${i}`).join(" ")));
});

test("duplicates end the window with the errors and KiCad's sentence", () => {
  const lines = planLines(plan({ errors: ["Duplicate instances of R3"] }), "footprint", mm);
  assert.deepEqual(lines.slice(-2).map((l) => [l.severity, l.text]), [
    ["error", "Duplicate instances of R3"],
    ["error", "Selected options resulted in errors! Change them and try again."],
  ]);
});

test("empty or invalid references are warned about and asked about before the run", () => {
  assert.deepEqual(badLines([], mm), []);
  assert.equal(confirmQuestion([]), null);
  const bad = [{ reference: "TP", fpid: "", x: 1000, y: 2000 }, { reference: "", fpid: "Lib:FP", x: 3000, y: 4000 }];
  assert.deepEqual(
    badLines(bad, mm).map((l) => l.text),
    [
      "PCB has 2 empty or invalid reference designations.",
      "You may wish to run DRC with 'Test for parity between PCB and schematic' checked.",
      "    RefDes: TP; footprint: unknown at 1.0 mm, 2.0 mm on PCB.",
      "    RefDes: unannotated footprint; footprint: Lib:FP at 3.0 mm, 4.0 mm on PCB.",
    ],
  );
  assert.ok(badLines(bad, mm).every((l) => l.severity === "warning"));
  assert.ok(confirmQuestion(bad)!.endsWith("Reannotate anyway?"));
});

test("after the run the selection follows the footprints to their new references", () => {
  assert.deepEqual(renamedSelection(["R1", "C9"], plan()), ["R2", "C9"]);
  assert.deepEqual(renamedSelection([], plan()), []);
  assert.equal(doneLines()[0]!.text, "PCB successfully reannotated");
});
