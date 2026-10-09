import { test } from "node:test";
import assert from "node:assert/strict";
import {
  addRecent,
  chosenOf,
  composeGroups,
  footprintFilterPatterns,
  groupKey,
  initialSelection,
  itemKey,
  keyOfItem,
  moveSelection,
  naturalCompare,
  orderGroups,
  parseQuery,
  passesFootprintFilters,
  PLACED_LABEL,
  RECENT_LABEL,
  scoreItemTerms,
  scoreItems,
  searchTerm,
  splitId,
  unitKey,
  urlIn,
  visibleRows,
  wildcardFull,
  type ChooserGroup,
  type ChooserItem,
  type ComposeInput,
} from "./libChooser";

const score = (query: string, terms: [string, number][]) =>
  scoreItemTerms(
    parseQuery(query),
    terms.map(([t, w]) => searchTerm(t, w))
  );

// ---- the same cases crates/cli/src/library_search.rs tests: the server and the browser score alike

test("a term scores 8 times its weight when exact, 2 times at its start and once inside", () => {
  assert.equal(score("lm358", [["LM358", 8]]), 1 + 8 * 8, "the whole term");
  assert.equal(score("lm3", [["LM358", 8]]), 1 + 2 * 8, "its start");
  assert.equal(score("358", [["LM358", 8]]), 1 + 8, "inside it");
  assert.equal(score("nope", [["LM358", 8]]), 0);
  assert.equal(
    score("opamp", [["Amplifier_Operational", 4], ["LM358", 8], ["dual opamp", 4], ["opamp", 4], ["Dual Operational Amplifier, opamp", 1]]),
    1 + 4 + 8 * 4 + 1
  );
});

test("every word of the query must score and the scores add", () => {
  const terms: [string, number][] = [["LM358", 8], ["Low-Power, Dual Operational Amplifier", 1], ["dual opamp", 4], ["dual", 4], ["opamp", 4]];
  assert.equal(score("lm358 dual", terms), 1 + 8 * 8 + (1 + 2 * 4 + 8 * 4));
  assert.equal(score("lm358 nothinglikeit", terms), 0, "one word that scores nothing hides the item");
  assert.equal(score("", terms), 1, "no query: every item shows");
  assert.equal(score("   ", terms), 1);
  assert.equal(score("LM358", terms), score("lm358", terms), "case does not matter");
});

test("a word may carry wildcards and nothing else is special", () => {
  const t: [string, number][] = [["SOIC-8_3.9x4.9mm_P1.27mm", 8]];
  assert.ok(score("soic*3.9x4.9", t) > 0);
  assert.ok(score("so?c", t) > 0);
  assert.ok(score("*p1.27", t) > 1, "a leading star matches from the start");
  assert.equal(score("soic*qfn", t), 0);
  assert.equal(score("p1*soic", t), 0, "the pieces must come in order");
  assert.equal(score("soic*3.9x4.9mm_p1.27mm", t), 1 + 2 * 8);
  assert.equal(score("/soic/", t), 0, "a regular expression is searched as the text it is");
  assert.equal(score("voltage>3.3", [["a voltage>3.3 b", 1]]), 1 + 1);
});

test("full wildcards anchor at both ends", () => {
  assert.ok(wildcardFull("soic*3.9x4.9mm*p1.27mm*", "soic-8_3.9x4.9mm_p1.27mm"));
  assert.ok(wildcardFull("dip*w7.62mm*", "dip-8_w7.62mm"));
  assert.ok(!wildcardFull("dip*w7.62mm", "dip-8_w7.62mm_socket"));
  assert.ok(wildcardFull("so?c", "soic"));
  assert.ok(!wildcardFull("so?c", "soc"));
  assert.ok(wildcardFull("*", ""));
  assert.ok(wildcardFull("a*b*c", "axxbyyc") && !wildcardFull("a*b*c", "axxbyy"));
});

test("natural order reads numbers as numbers and ignores case", () => {
  const names = ["R_0805", "r_0603", "R_10", "R_9", "Conn_02x03", "Conn_01x10", "Conn_01x2"];
  assert.deepEqual([...names].sort(naturalCompare), ["Conn_01x2", "Conn_01x10", "Conn_02x03", "R_9", "R_10", "r_0603", "R_0805"]);
  assert.equal(naturalCompare("abc", "ABC"), 0);
  assert.equal(naturalCompare("a", "ab"), -1, "the shorter first");
});

// ---- items

const sym = (id: string, description: string, extra: Partial<ChooserItem> = {}): ChooserItem => {
  const { lib, name } = splitId(id);
  return { id, lib, name, description, ...extra };
};

test("the terms of a symbol are the library, the name, Lib:Name, the keywords, the description, the footprint and the Description and Value columns", () => {
  const [item] = scoreItems("symbol", [sym("Amplifier_Operational:LM358", "Dual op-amp", { keywords: "dual opamp", value: "LM358", footprint: "Package_SO:SOIC-8" })], "lm358");
  // name 8 * 8, Lib:Name (inside) 16, Value column (exact) 8 * 4
  assert.equal(item!.score, 1 + 64 + 16 + 32);
  assert.equal(scoreItems("symbol", [sym("Device:R", "Resistor")], "resistor")[0]!.score, 1 + 8 * 1 + 8 * 4, "the description counts once as itself and once as the Description column");
  assert.equal(scoreItems("symbol", [sym("Device:R", "Resistor")], "zzz").length, 0);
  assert.equal(scoreItems("symbol", [sym("Device:R", "Resistor")], "")[0]!.score, 1, "no query: everything, with the base score");
  // a footprint has no Value column or default footprint
  assert.equal(scoreItems("footprint", [sym("Package_SO:SOIC-8", "SOIC, 8 Pin", { value: "SOIC-8" })], "soic-8")[0]!.score, 1 + 8 * 8 + 16 + 0);
});

// ---- the tree

const lm358 = sym("Amplifier_Operational:LM358", "Dual op-amp", { units: 3, pins: 8, score: 40 });
const r = sym("Device:R", "Resistor", { units: 1, pins: 2 });
const c = sym("Device:C", "Unpolarized capacitor", { units: 1, pins: 2 });

function input(extra: Partial<ComposeInput> = {}): ComposeInput {
  return {
    kind: "symbol",
    query: "",
    recent: [],
    installed: [{ name: "Device", count: undefined }, { name: "Amplifier_Operational" }, { name: "74xx" }, { name: "4xxx" }],
    loaded: new Map(),
    results: null,
    pinned: new Set(),
    ...extra,
  };
}

test("with no query the tree is: recently used, already placed, the project's libraries, then every installed library, collapsed", () => {
  const groups = composeGroups(
    input({
      recent: [r, lm358],
      placed: [sym("Zeta:Z", "z"), sym("Alpha:A2", "a"), sym("Alpha:A10", "a")],
      project: [sym("eda:MyPart", "mine"), sym("TEST:R", "t")],
    })
  );
  assert.deepEqual(groups.map((g) => g.lib), [RECENT_LABEL, PLACED_LABEL, "eda", "TEST", "4xxx", "74xx", "Amplifier_Operational", "Device"]);
  assert.deepEqual(groups[0]!.items.map((i) => i.id), ["Device:R", "Amplifier_Operational:LM358"], "recently used: by recency");
  assert.deepEqual(groups[1]!.items.map((i) => i.id), ["Alpha:A2", "Alpha:A10", "Zeta:Z"], "already placed: by name, numbers as numbers");
  const device = groups.find((g) => g.lib === "Device")!;
  assert.deepEqual([device.lazy, device.items.length, device.project ?? false], [true, 0, false], "an installed library nobody opened has no items yet");
  assert.equal(groups.find((g) => g.lib === "eda")!.project, true);
});

test("an installed library that was opened lists its items and is no longer lazy; pinned libraries come first", () => {
  const groups = composeGroups(input({ loaded: new Map([["Device", [r, c]]]), pinned: new Set(["Device", "74xx"]) }));
  assert.deepEqual(groups.map((g) => g.lib), ["74xx", "Device", "4xxx", "Amplifier_Operational"], "pinned first, each by name");
  const device = groups.find((g) => g.lib === "Device")!;
  assert.deepEqual([device.lazy, device.count, device.items.length], [false, 2, 2]);
});

test("a search keeps what scored: the browser's groups scored here, the installed libraries as the server answered, best library first", () => {
  const groups = composeGroups(
    input({
      query: "lm358",
      recent: [r, lm358],
      placed: [r],
      project: [sym("eda:LM358_Mine", "my lm358 variant"), sym("eda:Other", "nothing")],
      results: [
        { name: "Device", score: 12, items: [{ ...c, score: 12 }] },
        { name: "Amplifier_Operational", score: 73, items: [{ ...lm358, score: 73 }] },
      ],
    })
  );
  assert.deepEqual(groups.map((g) => g.lib), [RECENT_LABEL, "Amplifier_Operational", "eda", "Device"], "Already Placed lost its only symbol; eda keeps the one that scored; the libraries in order of their best item (73, 38, 12)");
  assert.deepEqual(groups[0]!.items.map((i) => i.id), ["Amplifier_Operational:LM358"]);
  assert.ok(groups[0]!.items[0]!.score! > 1);
  assert.deepEqual(groups.find((g) => g.lib === "eda")!.items.map((i) => i.id), ["eda:LM358_Mine"]);
  // an installed library already shown above (a project group of the same name) is not listed twice
  assert.equal(groups.filter((g) => g.lib === "Device").length, 1);
});

test("the group order is Compare's: recent, the other pseudo-libraries, pinned, then score (only while searching), then name", () => {
  const g = (lib: string, extra: Partial<ChooserGroup> = {}): ChooserGroup => ({ lib, pinned: false, items: [], lazy: false, score: 0, ...extra });
  const groups = [g("B", { score: 5 }), g("A", { score: 1 }), g("P", { pinned: true, score: 0 }), g(PLACED_LABEL, { pseudo: "placed" }), g(RECENT_LABEL, { pseudo: "recent" })];
  assert.deepEqual(orderGroups(groups, true).map((x) => x.lib), [RECENT_LABEL, PLACED_LABEL, "P", "B", "A"]);
  assert.deepEqual(orderGroups(groups, false).map((x) => x.lib), [RECENT_LABEL, PLACED_LABEL, "P", "A", "B"], "no search: by name");
});

test("rows: a group row, its items when it is open, a row per unit under an open multi-unit symbol", () => {
  const groups = composeGroups(input({ loaded: new Map([["Amplifier_Operational", [lm358]], ["Device", [r]]]) }));
  const closed = visibleRows(groups, { groups: new Set(), items: new Set(), searching: false });
  assert.deepEqual(closed.map((x) => x.kind), ["group", "group", "group", "group"]);
  const amp = groups.find((g) => g.lib === "Amplifier_Operational")!;
  const open = visibleRows(groups, { groups: new Set(["Amplifier_Operational"]), items: new Set([itemKey(amp, lm358)]), searching: false });
  assert.deepEqual(
    open.map((x) => x.kind + ":" + (x.kind === "group" ? x.group.lib : x.kind === "item" ? x.item.name : x.unit)),
    ["group:4xxx", "group:74xx", "group:Amplifier_Operational", "item:LM358", "unit:1", "unit:2", "unit:3", "group:Device"]
  );
  const item = open.find((x) => x.kind === "item")!;
  assert.deepEqual(item.kind === "item" && [item.expandable, item.open], [true, true]);
  // a single-unit symbol has no unit rows, and a footprint never does
  const flat = visibleRows(groups, { groups: new Set(["Device"]), items: new Set([itemKey(groups.find((g) => g.lib === "Device")!, r)]), searching: false });
  assert.equal(flat.some((x) => x.kind === "unit"), false);
  const fp = [{ lib: "Package_SO", pinned: false, items: [sym("Package_SO:SOIC-8", "x", { units: 5 })], lazy: false, score: 0 } as ChooserGroup];
  assert.equal(visibleRows(fp, { groups: new Set(["Package_SO"]), items: new Set([itemKey(fp[0]!, fp[0]!.items[0]!)]), searching: false }, "footprint").some((x) => x.kind === "unit"), false);
});

test("a search opens every group that has items", () => {
  const groups = composeGroups(input({ query: "lm", results: [{ name: "Amplifier_Operational", score: 40, items: [lm358] }] }));
  const rows = visibleRows(groups, { groups: new Set(), items: new Set(), searching: true });
  assert.deepEqual(rows.map((x) => x.kind), ["group", "item"]);
});

test("up and down move over the rows and stay inside", () => {
  const groups = composeGroups(input({ loaded: new Map([["Device", [r, c]]]) }));
  const rows = visibleRows(groups, { groups: new Set(["Device"]), items: new Set(), searching: false });
  const keys = rows.map((x) => x.key);
  assert.equal(moveSelection(rows, null, 1), keys[0], "nothing selected: the first row");
  assert.equal(moveSelection(rows, null, -1), keys[keys.length - 1], "up from nothing: the last");
  assert.equal(moveSelection(rows, keys[0]!, 1), keys[1]);
  assert.equal(moveSelection(rows, keys[0]!, -1), keys[0]);
  assert.equal(moveSelection(rows, keys[keys.length - 1]!, 1), keys[keys.length - 1]);
  assert.equal(moveSelection([], null, 1), null);
  assert.equal(moveSelection(rows, "gone", 1), keys[0]);
});

test("what OK chooses: an item or a unit of it, never a group", () => {
  const groups = composeGroups(input({ loaded: new Map([["Amplifier_Operational", [lm358]]]) }));
  const amp = groups.find((g) => g.lib === "Amplifier_Operational")!;
  const rows = visibleRows(groups, { groups: new Set(["Amplifier_Operational"]), items: new Set([itemKey(amp, lm358)]), searching: false });
  const by = (key: string) => rows.find((x) => x.key === key);
  assert.equal(chosenOf(by(groupKey(amp))), null);
  assert.deepEqual(chosenOf(by(itemKey(amp, lm358)))?.unit, 0);
  assert.deepEqual([chosenOf(by(unitKey(amp, lm358, 2)))?.item.id, chosenOf(by(unitKey(amp, lm358, 2)))?.unit], ["Amplifier_Operational:LM358", 2]);
  assert.equal(chosenOf(undefined), null);
});

test("when it opens: the best match of a search, else the item last used in its real library, else the only library's first item", () => {
  const best = { ...sym("Device:R", "r"), score: 50 };
  const groups = composeGroups(input({ query: "r", results: [{ name: "Device", score: 50, items: [{ ...c, score: 3 }, best] }] }));
  assert.equal(initialSelection(groups, "r", null).select, itemKey(groups.find((g) => g.lib === "Device")!, best), "the highest score, not the first");
  // a search whose best score is no more than the base still lands on the first thing shown
  const weak = composeGroups(input({ query: "x", results: [{ name: "Device", score: 1, items: [{ ...c, score: 1 }] }] }));
  assert.equal(initialSelection(weak, "x", null).select, itemKey(weak.find((g) => g.lib === "Device")!, weak.find((g) => g.lib === "Device")!.items[0]!));

  const recentGroups = composeGroups(input({ recent: [r], loaded: new Map([["Device", [c, r]]]) }));
  const sel = initialSelection(recentGroups, "", "Device:R");
  assert.deepEqual(sel.open, ["Device"], "the real library opens");
  assert.equal(sel.select, keyOfItem(recentGroups, "Device:R"));
  // the preselect is in the recent group only (its library never loaded): select it there
  const only = composeGroups(input({ recent: [lm358] }));
  assert.equal(initialSelection(only, "", lm358.id).select, itemKey(only[0]!, lm358));
  // nothing to preselect and one library: its first item
  const one = composeGroups(input({ installed: [{ name: "Device" }], loaded: new Map([["Device", [c, r]]]) }));
  assert.deepEqual(initialSelection(one, "", null), { open: ["Device"], select: itemKey(one[0]!, c) });
  // many libraries and nothing to preselect: nothing
  assert.deepEqual(initialSelection(composeGroups(input()), "", null), { open: [], select: null });
});

test("the recently used list keeps the last eight, newest first, once each", () => {
  let list: { id: string }[] = [];
  for (const id of ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]) list = addRecent(list, { id });
  assert.deepEqual(list.map((x) => x.id), ["j", "i", "h", "g", "f", "e", "d", "c"]);
  assert.deepEqual(addRecent(list, { id: "e" }).map((x) => x.id), ["e", "j", "i", "h", "g", "f", "d", "c"], "a repeat moves to the front");
});

// ---- footprints

test("footprint filters: the symbol's patterns match the name, or Lib:Name for a pattern with a colon", () => {
  const patterns = footprintFilterPatterns("SOIC*3.9x4.9mm*P1.27mm* DIP*W7.62mm*");
  assert.deepEqual(patterns, ["soic*3.9x4.9mm*p1.27mm*", "dip*w7.62mm*"]);
  assert.ok(passesFootprintFilters(patterns, "Package_SO", "SOIC-8_3.9x4.9mm_P1.27mm"));
  assert.ok(passesFootprintFilters(patterns, "Package_DIP", "DIP-8_W7.62mm"));
  assert.ok(!passesFootprintFilters(patterns, "Package_SO", "TSSOP-8_4.4x3mm_P0.65mm"));
  assert.ok(passesFootprintFilters([], "A", "B"), "no filters: everything passes");
  const qualified = footprintFilterPatterns(["Package_SO:SOIC*"]);
  assert.ok(passesFootprintFilters(qualified, "Package_SO", "SOIC-8"));
  assert.ok(!passesFootprintFilters(qualified, "Other", "SOIC-8"));
  assert.deepEqual(footprintFilterPatterns(undefined), []);
});

test("the documentation link of a footprint is the address in its description, without what ends the sentence", () => {
  assert.equal(urlIn("SOIC, 8 Pin (JEDEC MS-012AA, https://www.analog.com/media/pkg_pdf/r_8.pdf), generated with kicad-footprint-generator"), "https://www.analog.com/media/pkg_pdf/r_8.pdf");
  assert.equal(urlIn("See https://example.com/a.pdf."), "https://example.com/a.pdf");
  assert.equal(urlIn("(Body style from: https://this.url/part.pdf)"), "https://this.url/part.pdf");
  assert.equal(urlIn("no link here"), null);
});
