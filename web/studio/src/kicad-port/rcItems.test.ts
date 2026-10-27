import { test } from "node:test";
import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import type { DrcReport, DrcViolation, ErcReport, ErcViolation } from "../api/types";
import {
  SHOW_ALL,
  allShown,
  canExclude,
  countKinds,
  drcExclusionSpec,
  drcFixEntry,
  drcInspectEntry,
  drcKey,
  ercFixEntry,
  ercInspectEntry,
  isListed,
  listedIndexes,
  markerPrefix,
  menuSeverity,
  ofCheck,
  ofRule,
  patchDrcDeleteAll,
  patchDrcDeleteMarker,
  patchDrcExcluded,
  patchDrcSeverity,
  patchErcDeleteAll,
  patchErcDeleteMarker,
  ercKey,
  patchErcExcluded,
  patchErcSeverity,
  refilterDrc,
  rcKind,
  rcMenu,
  selectionAfterRowGone,
  setShowAll,
  severitiesAfter,
  stepListed,
  violatingRule,
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

test("the schematic's exclusions carry a comment too (m_ErcExclusionComments), so its menu has the comment entries as the board's does", () => {
  const ids = (excluded: boolean, comments?: boolean) => rcMenu({ domain: "erc", title: "Pin not connected", excluded, severity: "error", ...(comments === undefined ? {} : { comments }) }).map((e) => e.id);
  assert.deepEqual(ids(false).slice(0, 3), ["exclude", "exclude_comment", "exclude_type"]);
  assert.deepEqual(ids(true).slice(0, 3), ["remove_exclusion", "edit_comment", "remove_exclusion_type"]);
  assert.deepEqual(ids(false, false).slice(0, 2), ["exclude", "exclude_type"], "a menu can still be asked for without them");
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

test("the two placements of a shared screen have one location and their own sheets: waiving one leaves the other (p13-p16 F5)", () => {
  const on = (sheet: string): ErcViolation => ({ check: "pin_not_connected", severity: "error", location: "R1.2", hint: null, base_severity: "error", marker: { error_type: "ERCET_PIN_NOT_CONNECTED", at_nm: [1, 2], items: ["u"], sheet: ["root", sheet], main_sheet: ["root", sheet] } });
  const report: ErcReport = { violations: [on("s1"), on("s2")], counts: { pin_not_connected: 2 }, ignored_checks: [] };
  const key = ercKey(report.violations[1]!);
  assert.deepEqual(key, { check: "pin_not_connected", location: "R1.2", sheet: ["root", "s2"] });
  assert.deepEqual(patchErcExcluded(report, [key!], true).violations.map((x) => x.severity), ["error", "excluded"]);
  // a key with no sheet (a marker the report did not give) is the (check, location) alone
  assert.deepEqual(patchErcExcluded(report, [{ check: "pin_not_connected", location: "R1.2" }], true).violations.map((x) => x.severity), ["excluded", "excluded"]);
});

test("waiving a finding patches the schematic report on screen: severity, comment and counts, and restoring gives the severity back", () => {
  const e = (check: string, severity: ErcViolation["severity"], location: string | null, extra: Partial<ErcViolation> = {}): ErcViolation => ({ check, severity, location, hint: null, base_severity: severity === "error" ? "error" : "warning", ...extra });
  const report: ErcReport = {
    violations: [e("footprint_link_issues", "warning", "U1", { kicad_matched: false }), e("footprint_link_issues", "warning", "U2"), e("wire_dangling", "error", null)],
    counts: { footprint_link_issues: 2, wire_dangling: 1 },
    ignored_checks: [],
  };
  const waived = patchErcExcluded(report, [{ check: "footprint_link_issues", location: "U1" }], true, "bare footprint on purpose");
  assert.deepEqual(waived.violations.map((x) => x.severity), ["excluded", "warning", "error"]);
  assert.equal(waived.violations[0]!.comment, "bare footprint on purpose");
  assert.equal(waived.violations[0]!.kicad_matched, undefined, "whether kicad-cli matched the new exclusion is not known until the next run");
  assert.deepEqual(waived.counts, { footprint_link_issues: 1, wire_dangling: 1 });
  assert.equal(report.violations[0]!.severity, "warning", "the report it was given is not changed");
  // A finding with no location is nothing's key: it cannot be matched.
  assert.equal(patchErcExcluded(report, [{ check: "wire_dangling", location: "" }], true).violations[2]!.severity, "error");
  const back = patchErcExcluded(waived, [{ check: "footprint_link_issues", location: "U1" }], false);
  assert.deepEqual(back.violations.map((x) => x.severity), ["warning", "warning", "error"], "the severity it had when it was not excluded");
  assert.equal(back.violations[0]!.comment, "");
  assert.deepEqual(back.counts, { footprint_link_issues: 2, wire_dangling: 1 });
  // Editing the comment of one already waived keeps it waived.
  const edited = patchErcExcluded(waived, [{ check: "footprint_link_issues", location: "U1" }], true, "reviewed");
  assert.equal(edited.violations[0]!.severity, "excluded");
  assert.equal(edited.violations[0]!.comment, "reviewed");
  assert.deepEqual(edited.counts, waived.counts);
});

test("when an excluded row leaves the list the selection moves on to the next one, or back to the one before the last", () => {
  const listed = [1, 3, 4];
  assert.equal(selectionAfterRowGone(listed, 1), 3);
  assert.equal(selectionAfterRowGone(listed, 3), 4);
  assert.equal(selectionAfterRowGone(listed, 4), 3, "it was the last");
  assert.equal(selectionAfterRowGone([7], 7), null, "it was the only one");
  assert.equal(selectionAfterRowGone(listed, 2), null, "not in the list");
});

// ------------------------------------------------------------------------------------------------------------ the rule a violation broke

test("a custom rule is named in the violation's text, an implicit one is not a rule to exclude by", () => {
  assert.equal(violatingRule("Clearance violation (rule 'fine pitch'; actual 0.1 mm)"), "fine pitch");
  assert.equal(violatingRule("Track width (rule 'power tracks' min width 0.4 mm; actual 0.2 mm)"), "power tracks");
  assert.equal(violatingRule("Clearance violation (board setup constraints clearance 0.2 mm; actual 0.1 mm)"), null, "an implicit rule's name stands bare");
  assert.equal(violatingRule("Clearance violation (netclass 'Default' clearance 0.2 mm; actual 0.1 mm)"), null);
  assert.equal(violatingRule("Via annular width (rule ''; x)"), "", "an empty name is still the rule");
  assert.equal(violatingRule(""), null);
});

test("Exclude all violations of rule takes the violations of that rule on every page that are not waived yet, Remove all the ones that are", () => {
  const a = v("clearance", ["a", "b"], { description: "Clearance violation (rule 'fine pitch'; actual 0.1 mm)" });
  const b = v("track_width", ["t"], { description: "Track width (rule 'fine pitch' min width 0.4 mm; actual 0.2 mm)", excluded: true });
  const c = v("clearance", ["c", "d"], { description: "Clearance violation (rule 'other'; actual 0.1 mm)" });
  const d = v("clearance", ["e", "f"], { description: "Clearance violation (board setup constraints clearance 0.2 mm; actual 0.1 mm)" });
  const list = [a, b, c, d];
  assert.deepEqual(ofRule(list, "fine pitch", false), [a], "it is not the check that counts: the same check under another rule stays");
  assert.deepEqual(ofRule(list, "fine pitch", true), [b]);
  assert.deepEqual(ofRule(list, "other", false), [c]);
  assert.deepEqual(ofRule([{ ...a, items: [] }], "fine pitch", false), [], "a violation that names no item cannot be keyed");
});

test("the menu offers the rule's entries only for a custom rule, and the same entries to remove the exclusions", () => {
  const ids = (excluded: boolean, rule: string | null) => rcMenu({ domain: "drc", title: "Clearance violation", excluded, severity: "error", rule }).map((e) => e.id);
  assert.deepEqual(ids(false, "fine pitch").slice(0, 4), ["exclude", "exclude_comment", "exclude_rule", "exclude_type"]);
  assert.deepEqual(ids(true, "fine pitch").slice(0, 4), ["remove_exclusion", "edit_comment", "remove_exclusion_rule", "remove_exclusion_type"]);
  assert.ok(!ids(false, null).includes("exclude_rule"), "an implicit rule has no such entry");
  const entry = rcMenu({ domain: "drc", title: "x", excluded: false, severity: "error", rule: "fine pitch" }).find((e) => e.id === "exclude_rule")!;
  assert.equal(entry.label, "Exclude all violations of rule 'fine pitch'...");
  assert.equal(rcMenu({ domain: "drc", title: "x", excluded: true, severity: "warning", rule: "fine pitch" }).find((e) => e.id === "remove_exclusion_rule")!.label, "Remove all exclusions for violations of rule 'fine pitch'");
});

// ------------------------------------------------------------------------------------------------------------ Inspect and Fix

test("a DRC marker's Inspect entry is the resolution report of its check (InspectDRCErrorMenuText)", () => {
  for (const check of ["clearance", "copper_edge_clearance", "hole_clearance", "hole_to_hole", "starved_thermal"]) {
    assert.deepEqual(drcInspectEntry(check), { label: "Clearance Resolution", command: { kind: "inspect_clearance" } }, check);
  }
  for (const check of ["track_width", "via_diameter", "annular_width", "text_height", "connection_width", "assertion_failure", "diff_pair_uncoupled_length_too_long"]) {
    assert.deepEqual(drcInspectEntry(check), { label: "Constraints Resolution", command: { kind: "inspect_constraints" } }, check);
  }
  assert.deepEqual(drcInspectEntry("lib_footprint_mismatch"), { label: "Compare Footprint with Library", command: { kind: "action", id: "pcbnew.InspectionTool.DiffFootprint", select: true } });
  assert.equal(drcInspectEntry("silk_overlap"), null);
  assert.equal(drcInspectEntry("unconnected_items"), null);
});

test("every check the Inspect and Fix tables name is one KiCad reports under that key", () => {
  // The keys of `DRC_ITEM` / `ERC_ITEM` (`GetSettingsKey`): the pinned source's lists, plus the two kinds of finding that are always errors and so are not on the severity pages.
  const keys = (file: string): Set<string> => new Set([...readFileSync(`src/kicad/${file}`, "utf8").matchAll(/"key":\s*"([^"]+)"/g)].map((m) => m[1]!));
  const drc = keys("drc_checks.json");
  for (const always of ["padstack_invalid", "generic-warning", "generic-error"]) drc.add(always);
  const erc = keys("erc_checks.json");
  const named = [
    ...["clearance", "copper_edge_clearance", "hole_clearance", "hole_to_hole", "starved_thermal", "text_height", "text_thickness", "diff_pair_uncoupled_length_too_long", "track_width", "track_angle", "track_segment_length", "via_diameter", "annular_width", "drill_out_of_range", "microvia_drill_out_of_range", "connection_width", "assertion_failure", "lib_footprint_mismatch"],
    ...["lib_footprint_issues", "footprint_filters_mismatch", "footprint_symbol_mismatch", "footprint_symbol_field_mismatch", "missing_footprint", "duplicate_footprints", "extra_footprint", "footprint_type_mismatch", "footprint", "padstack", "padstack_invalid"],
    ...["mirrored_text_on_front_layer", "nonmirrored_text_on_back_layer", "track_dangling", "via_dangling"],
  ];
  for (const check of named) assert.ok(drc.has(check), `${check} is not a DRC check key`);
  // ... and each of them has the entry the tables give it, so a mistyped key shows here and not as a menu that has no entry.
  for (const check of named.slice(0, 18)) assert.ok(drcInspectEntry(check) !== null, `${check} has an Inspect entry`);
  for (const check of named.slice(18)) assert.ok(drcFixEntry(check, "part") !== null, `${check} has a Fix entry`);
  for (const check of ["simulation_model_issue", "footprint_filter", "footprint_link_issues", "lib_symbol_issues", "lib_symbol_mismatch", "unannotated", "duplicate_reference", "bus_to_net_conflict"]) {
    assert.ok(erc.has(check), `${check} is not an ERC check key`);
    assert.ok(ercFixEntry(check) !== null || ercInspectEntry(check) !== null, `${check} has an entry`);
  }
});

test("a DRC marker's Fix entry is the action that puts its check right (FixDRCErrorMenuText)", () => {
  assert.deepEqual(drcFixEntry("lib_footprint_issues", null), { label: "Manage Footprint Libraries...", command: { kind: "action", id: "common.SuiteControl.showFootprintLibTable", select: false } });
  assert.deepEqual(drcFixEntry("lib_footprint_mismatch", null), { label: "Update Footprint...", command: { kind: "action", id: "pcbnew.GlobalEdit.updateFootprint", select: true } });
  assert.deepEqual(drcFixEntry("footprint_filters_mismatch", null), { label: "Change Footprint...", command: { kind: "action", id: "pcbnew.GlobalEdit.changeFootprint", select: true } });
  for (const check of ["footprint_symbol_mismatch", "footprint_symbol_field_mismatch", "missing_footprint", "duplicate_footprints", "extra_footprint"]) {
    assert.deepEqual(drcFixEntry(check, null), { label: "Update PCB from Schematic...", command: { kind: "action", id: "common.Control.updatePcbFromSchematic", select: false } }, check);
  }
  assert.equal(drcFixEntry("footprint", "part")!.label, "Edit Footprint Properties...");
  assert.equal(drcFixEntry("footprint_type_mismatch", "part")!.label, "Edit Footprint Properties...");
  assert.equal(drcFixEntry("padstack", "pad")!.label, "Edit Pad Properties...");
  assert.deepEqual(drcFixEntry("track_dangling", null)!.command, { kind: "action", id: "pcbnew.GlobalEdit.cleanupTracksAndVias", select: false });
  assert.equal(drcFixEntry("via_dangling", null)!.label, "Cleanup Tracks & Vias...");
  // A text check names what the text is: a dimension, a footprint field or a text.
  assert.equal(drcFixEntry("text_height", "dimension")!.label, "Edit Dimension Properties...");
  assert.equal(drcFixEntry("text_thickness", "field")!.label, "Edit Field Properties...");
  assert.equal(drcFixEntry("mirrored_text_on_front_layer", "text")!.label, "Edit Text Properties...");
  assert.equal(drcFixEntry("nonmirrored_text_on_back_layer", null)!.label, "Edit Text Properties...");
  assert.deepEqual(drcFixEntry("text_height", "text")!.command, { kind: "properties" });
  assert.equal(drcFixEntry("clearance", "part"), null);
});

test("an ERC marker's Inspect and Fix entries (InspectERCErrorMenuText, FixERCErrorMenuText)", () => {
  assert.deepEqual(ercInspectEntry("bus_to_net_conflict"), { label: "Show Bus Syntax Help", command: { kind: "action", id: "eeschema.InspectionTool.showBusSyntaxHelp", select: false } });
  assert.equal(ercInspectEntry("lib_symbol_mismatch")!.label, "Compare Symbol with Library");
  assert.equal(ercInspectEntry("pin_not_connected"), null);
  for (const check of ["simulation_model_issue", "footprint_filter", "footprint_link_issues"]) {
    assert.deepEqual(ercFixEntry(check), { label: "Edit Symbol Properties...", command: { kind: "action", id: "eeschema.InteractiveEdit.properties", select: true } }, check);
  }
  assert.equal(ercFixEntry("lib_symbol_issues")!.label, "Manage Symbol Libraries...");
  assert.equal(ercFixEntry("lib_symbol_mismatch")!.label, "Update Symbol...");
  for (const check of ["unannotated", "duplicate_reference"]) assert.deepEqual(ercFixEntry(check)!.command, { kind: "action", id: "eeschema.EditorControl.annotate", select: false });
  assert.equal(ercFixEntry("undefined_netclass"), null, "Net Classes is a page of Schematic Setup this app does not have");
  assert.equal(ercFixEntry("wire_dangling"), null);
});

test("the marker menu puts Inspect and Fix between separators after the exclusions, and the connection grid after the severities", () => {
  const menu = rcMenu({
    domain: "drc",
    title: "Clearance violation",
    excluded: false,
    severity: "error",
    inspect: drcInspectEntry("clearance"),
    fix: drcFixEntry("footprint", "part"),
  });
  assert.deepEqual(
    menu.map((e) => e.id),
    ["exclude", "exclude_comment", "exclude_type", "separator", "inspect", "fix", "separator", "severity_warning", "severity_ignore", "separator", "edit_severities"]
  );
  assert.equal(menu.find((e) => e.id === "inspect")!.label, "Clearance Resolution");
  assert.ok(!rcMenu({ domain: "drc", title: "x", excluded: false, severity: "error" }).some((e) => e.id === "inspect" || e.id === "fix"), "no entries, no separator pair");
  assert.deepEqual(
    rcMenu({ domain: "drc", title: "x", excluded: false, severity: "error" }).filter((e) => e.id === "separator").length,
    2
  );
  const off = rcMenu({ domain: "erc", title: "Symbol pin or wire end off connection grid", excluded: false, severity: "warning", connectionGrid: true });
  assert.deepEqual(off.slice(-2).map((e) => e.id), ["edit_severities", "connection_grid"]);
  assert.equal(off.at(-1)!.label, "Edit connection grid spacing...");
  assert.ok(!rcMenu({ domain: "erc", title: "x", excluded: false, severity: "warning" }).some((e) => e.id === "connection_grid"));
  assert.ok(rcMenu({ domain: "erc", title: "x", excluded: false, severity: "warning", connectionGrid: true, onCanvas: true }).at(-1)!.id === "show_in_dialog", "the canvas marker's own entry stays last");
});

// ------------------------------------------------------------------------------------------------------------ Delete Marker, Delete All Markers

test("Delete Marker takes one marker off a page of the DRC report and the counts follow; nothing else changes", () => {
  const report: DrcReport = {
    violations: [v("clearance", ["a", "b"]), v("clearance", ["c", "d"], { excluded: true }), v("track_width", ["t"])],
    unconnected_items: [v("unconnected_items", ["p", "q"]), v("unconnected_items", ["r", "s"])],
    schematic_parity: [v("missing_footprint", ["m"])],
    counts: { clearance: 1, track_width: 1, unconnected_items: 2, missing_footprint: 1 },
    ignored_checks: [{ key: "silk_overlap", description: "Silkscreen overlap" }],
  };
  const gone = patchDrcDeleteMarker(report, "violations", 0);
  assert.deepEqual(gone.violations.map((x) => x.items[0]!.uuid), ["c", "t"]);
  assert.deepEqual(gone.counts, { track_width: 1, unconnected_items: 2, missing_footprint: 1 }, "the deleted one is not counted any more");
  assert.equal(gone.unconnected_items!.length, 2, "the other pages are untouched");
  assert.equal(gone.markers_deleted, undefined, "one marker is not Delete All");
  assert.deepEqual(gone.ignored_checks, report.ignored_checks);
  // An excluded marker can be deleted too: it leaves the list (its exclusion is the design's, and goes with it through `deleteMarkerDrc`'s `delete_drc_exclusions`, not here).
  assert.deepEqual(patchDrcDeleteMarker(gone, "violations", 0).violations.map((x) => x.items[0]!.uuid), ["t"]);
  const unconnected = patchDrcDeleteMarker(report, "unconnected", 1);
  assert.deepEqual(unconnected.unconnected_items!.map((x) => x.items[0]!.uuid), ["p"]);
  assert.deepEqual(unconnected.counts, { clearance: 1, track_width: 1, unconnected_items: 1, missing_footprint: 1 });
  assert.equal(patchDrcDeleteMarker(report, "parity", 0).schematic_parity!.length, 0);
  assert.equal(report.violations.length, 3, "the report it was given is not changed");
});

test("Delete All Markers empties every page of errors and warnings, and of exclusions when it is asked to, and leaves the report to be run again", () => {
  const report: DrcReport = {
    violations: [v("clearance", ["a", "b"]), v("clearance", ["c", "d"], { excluded: true })],
    unconnected_items: [v("unconnected_items", ["p", "q"], { excluded: true }), v("unconnected_items", ["r", "s"])],
    schematic_parity: [v("missing_footprint", ["m"])],
    counts: { clearance: 1, unconnected_items: 1, missing_footprint: 1 },
  };
  const some = patchDrcDeleteAll(report, false);
  assert.deepEqual(some.violations.map((x) => x.items[0]!.uuid), ["c"], "the exclusions stay");
  assert.deepEqual(some.unconnected_items!.map((x) => x.items[0]!.uuid), ["p"]);
  assert.equal(some.schematic_parity!.length, 0);
  assert.deepEqual(some.counts, {});
  assert.equal(some.markers_deleted, true);
  const all = patchDrcDeleteAll(report, true);
  assert.equal(all.violations.length + all.unconnected_items!.length + all.schematic_parity!.length, 0);
  assert.equal(all.markers_deleted, true);
});

test("the ERC report loses a marker the same way, and the exclusions only when they are asked for", () => {
  const e = (check: string, severity: ErcViolation["severity"], location: string): ErcViolation => ({ check, severity, location, hint: null });
  const report: ErcReport = { violations: [e("pin_not_connected", "error", "U1.1"), e("pin_not_connected", "excluded", "U1.2"), e("label_dangling", "warning", "L1")], counts: { pin_not_connected: 1, label_dangling: 1 } };
  const one = patchErcDeleteMarker(report, 1);
  assert.deepEqual(one.violations.map((x) => x.location), ["U1.1", "L1"]);
  assert.deepEqual(one.counts, { pin_not_connected: 1, label_dangling: 1 });
  assert.deepEqual(patchErcDeleteMarker(report, 0).counts, { label_dangling: 1 });
  const some = patchErcDeleteAll(report, false);
  assert.deepEqual(some.violations.map((x) => x.location), ["U1.2"]);
  assert.deepEqual(some.counts, {});
  assert.equal(some.markers_deleted, true);
  assert.equal(patchErcDeleteAll(report, true).violations.length, 0);
});

test("r2 D-E3: the list is every error, then the warnings, then the exclusions, each in report order (SetSeverities' stable sort)", () => {
  const mixed = [v("w", ["1"], { severity: "warning" }), v("e", ["2"]), v("x", ["3"], { excluded: true }), v("e2", ["4"])];
  assert.deepEqual(listedIndexes(mixed, SHOW_ALL), [1, 3, 0, 2]);
  assert.deepEqual(listedIndexes(mixed, { errors: false, warnings: true, exclusions: true }), [0, 2]);
});

test("r2 D-E3: a DRC marker waived or restored on its own keeps its row until the next refilter; a bulk change or a new severity sorts again", () => {
  const report: DrcReport = { violations: [v("e1", ["1"]), v("w", ["2"], { severity: "warning" }), v("e2", ["3"])], counts: {} } as unknown as DrcReport;
  assert.deepEqual(listedIndexes(report.violations, SHOW_ALL), [0, 2, 1]);
  // DIALOG_DRC's ID_ADD_EXCLUSION / ExcludeMarker: `ValueChanged( node )`, no refilter
  const held = patchDrcExcluded(report, [{ check: "e1", items: ["1"] }], true, "", true);
  assert.deepEqual(listedIndexes(held.violations, SHOW_ALL), [0, 2, 1], "the waived error keeps the first row");
  const back = patchDrcExcluded(held, [{ check: "e1", items: ["1"] }], false, "", true);
  assert.deepEqual(listedIndexes(back.violations, SHOW_ALL), [0, 2, 1]);
  // ID_ADD_EXCLUSION_ALL and friends: `Update( m_markersProvider, getSeverities() )`, which refilters and sorts
  const bulk = patchDrcExcluded(held, [{ check: "e2", items: ["3"] }], true, "", false);
  assert.deepEqual(listedIndexes(bulk.violations, SHOW_ALL), [1, 0, 2]);
  const sev = patchDrcSeverity(held, "w", "error", "w");
  assert.deepEqual(listedIndexes(sev.violations, SHOW_ALL), [1, 2, 0], "a severity change refilters: the waived row goes to the end");
  assert.deepEqual(listedIndexes(refilterDrc(held).violations, SHOW_ALL), [2, 1, 0], "a change of the Show boxes refilters too");
  // with exclusions hidden a waived row leaves the list at once (`DeleteCurrentItem`)
  assert.deepEqual(listedIndexes(held.violations, { errors: true, warnings: true, exclusions: false }), [2, 1]);
});
