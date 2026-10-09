import { test } from "node:test";
import assert from "node:assert/strict";
import type { DrcReport, DrcViolation, ErcReport, ErcViolation } from "../api/types";
import {
  SHOW_ALL,
  allShown,
  canExclude,
  countKinds,
  drcExclusionSpec,
  drcKey,
  isListed,
  listedIndexes,
  markerPrefix,
  menuSeverity,
  ofCheck,
  patchDrcExcluded,
  patchDrcSeverity,
  patchErcSeverity,
  rcKind,
  rcMenu,
  selectionAfterRowGone,
  setShowAll,
  severitiesAfter,
  stepListed,
} from "./rcItems";

const v = (type: string, uuids: string[], extra: Partial<DrcViolation> = {}): DrcViolation => ({
  type,
  description: type,
  severity: "error",
  items: uuids.map((u, i) => ({ description: `item ${i}`, pos: [i * 1000, 0], id: `id-${u}`, uuid: u })),
  ...extra,
});

test("a marker is listed by what it is: an excluded one is an exclusion whatever its severity", () => {
  assert.equal(rcKind({ severity: "error" }), "error");
  assert.equal(rcKind({ severity: "warning" }), "warning");
  assert.equal(rcKind({ severity: "warning", excluded: true }), "exclusion");
  assert.equal(rcKind({ severity: "excluded" }), "exclusion", "the ERC report folds the two together");
  assert.equal(isListed("exclusion", { errors: true, warnings: true, exclusions: false }), false);
  assert.equal(isListed("warning", { errors: true, warnings: false, exclusions: true }), false);
  const list = [v("a", ["1"]), v("b", ["2"], { severity: "warning" }), v("c", ["3"], { excluded: true }), v("d", ["4"], { severity: "warning", excluded: true })];
  assert.deepEqual(listedIndexes(list, SHOW_ALL), [0, 1, 2, 3]);
  assert.deepEqual(listedIndexes(list, { errors: true, warnings: false, exclusions: false }), [0]);
  assert.deepEqual(listedIndexes(list, { errors: false, warnings: true, exclusions: true }), [1, 2, 3]);
});

test("the All box turns warnings and exclusions on or off with it and leaves the errors on", () => {
  assert.deepEqual(setShowAll(false), { errors: true, warnings: false, exclusions: false });
  assert.deepEqual(setShowAll(true), SHOW_ALL);
  assert.equal(allShown(SHOW_ALL), true);
  assert.equal(allShown({ errors: true, warnings: true, exclusions: false }), false);
});

test("the counts are of every marker, whatever the Show boxes say, and the exclusions are their own", () => {
  const a = [v("a", ["1"]), v("b", ["2"], { severity: "warning" })];
  const b = [v("c", ["3"], { excluded: true }), v("d", ["4"])];
  assert.deepEqual(countKinds(a, b), { errors: 2, warnings: 1, exclusions: 1 });
  assert.deepEqual(countKinds(), { errors: 0, warnings: 0, exclusions: 0 });
});

test("a marker's line says what it is, and an excluded one what it would have been", () => {
  assert.equal(markerPrefix("error", "error"), "Error: ");
  assert.equal(markerPrefix("warning", "warning"), "Warning: ");
  assert.equal(markerPrefix("exclusion", "warning"), "Excluded warning: ");
  assert.equal(markerPrefix("exclusion", "error"), "Excluded error: ");
  assert.equal(markerPrefix("exclusion", undefined), "Excluded error: ");
});

test("Next and Previous Marker step the listed markers, not the hidden ones", () => {
  // The report holds 6 markers; the Show boxes list the 2nd, 4th and 5th (indexes 1, 3, 4).
  const listed = [1, 3, 4];
  assert.equal(stepListed(listed, null, "next"), 1, "none selected: the first one listed");
  assert.equal(stepListed(listed, 1, "next"), 3, "the hidden marker between them is skipped");
  assert.equal(stepListed(listed, 3, "next"), 4);
  assert.equal(stepListed(listed, 4, "next"), null, "nothing after the last: the selection stays");
  assert.equal(stepListed(listed, null, "prev"), 4, "none selected: the last one listed is the one before");
  assert.equal(stepListed(listed, 4, "prev"), 3);
  assert.equal(stepListed(listed, 1, "prev"), null, "nothing before the first");
  assert.equal(stepListed(listed, 0, "next"), 1, "a selection the list does not show counts as none");
  assert.equal(stepListed([], null, "next"), null);
});

test("the dialog's marker menu follows OnDRCItemRClick: exclude, change the severity, edit the severities", () => {
  const ids = (e: ReturnType<typeof rcMenu>) => e.map((x) => x.id);
  const fresh = rcMenu({ domain: "drc", title: "Clearance violation", excluded: false, severity: "error" });
  assert.deepEqual(ids(fresh), ["exclude", "exclude_comment", "exclude_type", "separator", "severity_warning", "severity_ignore", "separator", "edit_severities"]);
  assert.equal(fresh[0]!.label, "Exclude this violation");
  assert.equal(fresh[0]!.hint, "It will be excluded from the errors list");
  assert.equal(fresh[2]!.label, "Exclude all 'Clearance violation' violations");
  assert.equal(fresh[4]!.label, "Change severity to Warning for all 'Clearance violation' violations");
  assert.equal(fresh[7]!.hint, "Open the Board Setup dialog");

  const waived = rcMenu({ domain: "drc", title: "Clearance violation", excluded: true, severity: "warning" });
  assert.deepEqual(ids(waived), ["remove_exclusion", "edit_comment", "remove_exclusion_type", "separator", "severity_error", "severity_ignore", "separator", "edit_severities"]);
  assert.equal(waived[0]!.hint, "It will be placed back in the warnings list");
  assert.equal(waived[4]!.label, "Change severity to Error for all 'Clearance violation' violations", "a warning can become an error");
});

test("the canvas marker's menu is the dialog's and also shows the marker in the dialog", () => {
  const menu = rcMenu({ domain: "erc", title: "Pin not connected", excluded: false, severity: "error", onCanvas: true });
  assert.equal(menu[menu.length - 1]!.id, "show_in_dialog");
  assert.equal(menu[menu.length - 1]!.label, "Show in the Electrical Rules Checker");
  assert.equal(menu[menu.length - 3]!.hint, "Open the Schematic Setup dialog");
  assert.equal(rcMenu({ domain: "drc", title: "x", excluded: false, severity: "error", onCanvas: true }).at(-1)!.label, "Show in the Design Rules Checker");
  assert.ok(!rcMenu({ domain: "drc", title: "x", excluded: false, severity: "error" }).some((e) => e.id === "show_in_dialog"), "the dialog's own menu has no use for it");
});

test("the pin-to-pin conflict check's severity is the pin map's: only Ignore is a choice, and the setup is the map", () => {
  const menu = rcMenu({ domain: "erc", title: "Conflict problem between pins", excluded: false, severity: "warning", pinMap: true });
  assert.deepEqual(
    menu.map((e) => e.id),
    ["exclude", "exclude_comment", "exclude_type", "separator", "severity_ignore", "separator", "edit_severities"]
  );
  assert.equal(menu.at(-1)!.label, "Edit pin-to-pin conflict map...");
});

test("the schematic's exclusions carry no comment, so its menu has no comment entries", () => {
  const ids = (excluded: boolean) => rcMenu({ domain: "erc", title: "Pin not connected", excluded, severity: "error", comments: false }).map((e) => e.id);
  assert.deepEqual(ids(false).slice(0, 2), ["exclude", "exclude_type"]);
  assert.deepEqual(ids(true).slice(0, 2), ["remove_exclusion", "remove_exclusion_type"]);
});

test("a menu entry maps to the severity it sets", () => {
  assert.equal(menuSeverity("severity_error"), "error");
  assert.equal(menuSeverity("severity_warning"), "warning");
  assert.equal(menuSeverity("severity_ignore"), "ignore");
  assert.equal(menuSeverity("exclude"), null);
});

test("a severity change sends the checks that differ from KiCad's default, and the ones the table names already", () => {
  const items = [
    { key: "clearance", defaultSeverity: "error" as const },
    { key: "silk_overlap", defaultSeverity: "warning" as const },
    { key: "lib_footprint_issues", defaultSeverity: "warning" as const },
  ];
  // The table names the library check (this app ignores it unless told otherwise).
  const current = { lib_footprint_issues: "ignore" as const };
  assert.deepEqual(severitiesAfter(items, current, "clearance", "warning"), { clearance: "warning", lib_footprint_issues: "ignore" });
  assert.deepEqual(severitiesAfter(items, current, "silk_overlap", "ignore"), { silk_overlap: "ignore", lib_footprint_issues: "ignore" });
  assert.deepEqual(severitiesAfter(items, { clearance: "warning", lib_footprint_issues: "ignore" }, "clearance", "error"), { clearance: "error", lib_footprint_issues: "ignore" }, "back to the default, said out loud: the table named the check already");
  assert.deepEqual(severitiesAfter(items, {}, "clearance", "error"), {}, "a check at its default that the table never named is not sent");
  assert.deepEqual(severitiesAfter(items, current, "lib_footprint_issues", "warning"), { lib_footprint_issues: "warning" }, "the default said out loud, since the table named it");
});

test("a violation is waived by its check and the uuids of its items, with the positions its marker may sit at", () => {
  const one = v("annular_width", ["via-1"], { marker_nm: [[12_459_000, 18_390_000]] });
  assert.deepEqual(drcKey(one), { check: "annular_width", items: ["via-1"] });
  assert.deepEqual(drcExclusionSpec(one), { check: "annular_width", items: ["via-1"], ids: ["id-via-1"], positions_nm: [[12_459_000, 18_390_000]] });
  assert.deepEqual(drcExclusionSpec(v("clearance", ["a", "b"]), "slot on purpose"), { check: "clearance", items: ["a", "b"], ids: ["id-a", "id-b"], comment: "slot on purpose" });
  // An item that is not one of ours has no id: an empty one keeps the lists the same length.
  const foreign: DrcViolation = { ...v("clearance", ["a", "z"]), items: [{ description: "x", pos: [0, 0], id: "R1.1", uuid: "a" }, { description: "fill", pos: [0, 0], id: null, uuid: "z" }] };
  assert.deepEqual(drcExclusionSpec(foreign).ids, ["R1.1", ""]);
  assert.equal(canExclude(one), true);
  assert.equal(canExclude({ items: [] }), false, "a finding that names no item");
  assert.equal(canExclude({ items: [{ description: "x", pos: [0, 0], id: null }] }), false, "a lint finding, or a report from an older server: no uuid");
});

test("Exclude All of a check takes the violations of it that are not waived; Remove All the ones that are", () => {
  const list = [v("clearance", ["a", "b"]), v("clearance", ["a", "c"], { excluded: true }), v("hole_clearance", ["d"]), v("clearance", ["e", "f"])];
  assert.deepEqual(ofCheck(list, "clearance", false).map((x) => drcKey(x).items.join("+")), ["a+b", "e+f"]);
  assert.deepEqual(ofCheck(list, "clearance", true).map((x) => drcKey(x).items.join("+")), ["a+c"]);
  assert.deepEqual(ofCheck(list, "nothing", false), []);
});

test("waiving a violation patches the report on screen: flags, comment, and counts without the waived", () => {
  const report: DrcReport = {
    violations: [v("clearance", ["a", "b"], { kicad_matched: false }), v("clearance", ["a", "c"]), v("annular_width", ["via"], { excluded: true, comment: "old", kicad_matched: true })],
    unconnected_items: [v("unconnected_items", ["p", "q"])],
    schematic_parity: [v("net_conflict", ["p"], { severity: "warning" })],
    counts: { clearance: 2, unconnected_items: 1, net_conflict: 1 },
  };
  const after = patchDrcExcluded(report, [{ check: "clearance", items: ["a", "b"] }, { check: "unconnected_items", items: ["p", "q"] }], true, "fine");
  assert.equal(after.violations[0]!.excluded, true);
  assert.equal(after.violations[0]!.comment, "fine");
  assert.equal(after.violations[0]!.kicad_matched, undefined, "whether kicad-cli matches the new exclusion is not known until the next run");
  assert.equal(after.violations[1]!.excluded, undefined, "the other clearance is untouched");
  assert.equal(after.unconnected_items![0]!.excluded, true);
  assert.deepEqual(after.counts, { clearance: 1, net_conflict: 1 });
  assert.equal(report.violations[0]!.excluded, undefined, "the report it was given is not changed");

  const back = patchDrcExcluded(after, [{ check: "annular_width", items: ["via"] }], false);
  assert.equal(back.violations[2]!.excluded, false);
  assert.equal(back.violations[2]!.comment, "");
  assert.deepEqual(back.counts, { clearance: 1, net_conflict: 1, annular_width: 1 });
  // The order of the items is part of the key.
  assert.equal(patchDrcExcluded(report, [{ check: "clearance", items: ["b", "a"] }], true).violations[0]!.excluded, undefined);
});

test("a check set to Ignore leaves the report and joins the ignored tests; a check set to a severity keeps its markers at it", () => {
  const report: DrcReport = {
    violations: [v("clearance", ["a", "b"]), v("clearance", ["a", "c"], { excluded: true }), v("silk_overlap", ["d"], { severity: "warning" })],
    unconnected_items: [v("unconnected_items", ["p", "q"])],
    counts: { clearance: 1, silk_overlap: 1, unconnected_items: 1 },
    ignored_checks: [{ key: "missing_courtyard", description: "Footprint has no courtyard defined" }],
  };
  const ignored = patchDrcSeverity(report, "clearance", "ignore", "Clearance violation");
  assert.deepEqual(ignored.violations.map((x) => x.type), ["silk_overlap"], "its markers go, the waived one too");
  assert.deepEqual(ignored.counts, { silk_overlap: 1, unconnected_items: 1 });
  assert.deepEqual(ignored.ignored_checks, [{ key: "missing_courtyard", description: "Footprint has no courtyard defined" }, { key: "clearance", description: "Clearance violation" }]);

  const warned = patchDrcSeverity(report, "clearance", "warning", "Clearance violation");
  assert.deepEqual(warned.violations.map((x) => [x.type, x.severity]), [["clearance", "warning"], ["clearance", "warning"], ["silk_overlap", "warning"]]);
  assert.equal(warned.violations[1]!.excluded, true, "a waived marker stays waived");
  assert.equal(warned.ignored_checks!.length, 1);
  assert.deepEqual(warned.counts, { clearance: 1, silk_overlap: 1, unconnected_items: 1 });

  // Taking a check off Ignore (the Ignored Tests tab's radio menu) removes it from that list.
  const back = patchDrcSeverity(report, "missing_courtyard", "warning", "Footprint has no courtyard defined");
  assert.deepEqual(back.ignored_checks, []);
  assert.equal(report.violations.length, 3, "the report it was given is not changed");
});

test("the schematic report takes a severity change the same way and keeps its excluded findings excluded", () => {
  const e = (check: string, severity: ErcViolation["severity"], location: string): ErcViolation => ({ check, severity, location, hint: null });
  const report: ErcReport = { violations: [e("pin_not_connected", "error", "U1.1"), e("pin_not_connected", "excluded", "U1.2"), e("label_dangling", "warning", "L1")], counts: { pin_not_connected: 1, label_dangling: 1 }, ignored_checks: [] };
  const warned = patchErcSeverity(report, "pin_not_connected", "warning", "Pin not connected");
  assert.deepEqual(warned.violations.map((x) => x.severity), ["warning", "excluded", "warning"]);
  assert.deepEqual(warned.counts, { pin_not_connected: 1, label_dangling: 1 });
  const off = patchErcSeverity(report, "pin_not_connected", "ignore", "Pin not connected");
  assert.deepEqual(off.violations.map((x) => x.check), ["label_dangling"]);
  assert.deepEqual(off.counts, { label_dangling: 1 });
  assert.deepEqual(off.ignored_checks, [{ key: "pin_not_connected", description: "Pin not connected" }]);
});

test("when an excluded row leaves the list the selection moves on to the next one, or back to the one before the last", () => {
  const listed = [1, 3, 4];
  assert.equal(selectionAfterRowGone(listed, 1), 3);
  assert.equal(selectionAfterRowGone(listed, 3), 4);
  assert.equal(selectionAfterRowGone(listed, 4), 3, "it was the last");
  assert.equal(selectionAfterRowGone([7], 7), null, "it was the only one");
  assert.equal(selectionAfterRowGone(listed, 2), null, "not in the list");
});
