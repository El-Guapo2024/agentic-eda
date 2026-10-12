import { test } from "node:test";
import assert from "node:assert/strict";
import { appliable, backAnnotateCommands, backAnnotateLines, changeLines, updateCommands, updatePcbMessage } from "./updatePcb";
import { CHANGE_OPTIONS, REPLACE_OPTIONS, UPDATE_OPTIONS, exchangeReport, isLibraryId, matchingFootprints, setAllOptions } from "./pcbExchange";
import type { BoardState } from "../api/types";

const change = (reference: string, from: string, to: string, extra: Partial<{ locked: boolean; found: boolean }> = {}) => ({ reference, from, to, locked: false, found: true, ...extra });

test("F8 lists a footprint change as KiCad's dry run does, and tells it in the past after the update", () => {
  const changes = [change("R1", "R_0603", "R_0805"), change("R2", "R_0603", "R_0402", { locked: true }), change("R3", "R_0603", "Nope:R", { found: false })];
  assert.deepEqual(changeLines(changes, false).map((l) => [l.severity, l.text]), [
    ["action", "Change R1 footprint from 'R_0603' to 'R_0805'."],
    ["warning", "Cannot change R2 footprint from 'R_0603' to 'R_0402' (footprint is locked)."],
    ["error", "Cannot update R3 (footprint 'Nope:R' not found)."],
  ]);
  assert.equal(changeLines(changes, true)[0]!.text, "Changed R1 footprint from 'R_0603' to 'R_0805'.");
  assert.equal(changeLines(changes, true)[1]!.text, "Could not change R2 footprint from 'R_0603' to 'R_0402' (footprint is locked).");
});

test("the update applies the footprints that can be changed, each with every option on", () => {
  const changes = [change("R1", "A", "B"), change("R2", "A", "B", { locked: true }), change("R3", "A", "C", { found: false })];
  assert.deepEqual(appliable(changes).map((c) => c.reference), ["R1"]);
  assert.deepEqual(updateCommands(changes), [{ op: "exchange_footprints", references: ["R1"], footprint: "B", options: REPLACE_OPTIONS }]);
  assert.deepEqual(updateCommands([]), []);
});

test("the back-annotation names the board's footprint on the symbol", () => {
  const changes = [change("R1", "Lib:A", "Lib:B"), change("R2", "", "Lib:B")];
  assert.deepEqual(backAnnotateCommands(changes), [{ op: "edit_symbol_fields", id: "R1", footprint: "Lib:A" }]);
  assert.deepEqual(backAnnotateLines(changes, false).map((l) => l.text), ["Change R1 footprint assignment from 'Lib:B' to 'Lib:A'."]);
});

test("the F8 summary says what is left to place", () => {
  assert.match(updatePcbMessage([]), /no footprints yet/);
  assert.match(updatePcbMessage([{ ref: "R1", placed: true }]), /1 footprint, all placed/);
  assert.match(updatePcbMessage([{ ref: "R1", placed: true }, { ref: "R2", placed: false }]), /1 placed, 1 still to place \(R2\)/);
});

test("the dialog opens with KiCad's option values for a change and for an update", () => {
  assert.equal(CHANGE_OPTIONS.remove_extra_texts, false);
  assert.deepEqual([CHANGE_OPTIONS.reset_text_layers, CHANGE_OPTIONS.reset_text_effects, CHANGE_OPTIONS.reset_text_positions, CHANGE_OPTIONS.reset_text_content, CHANGE_OPTIONS.reset_fabrication_attrs], [true, true, true, true, true]);
  assert.deepEqual([UPDATE_OPTIONS.reset_text_layers, UPDATE_OPTIONS.reset_text_effects, UPDATE_OPTIONS.reset_text_positions, UPDATE_OPTIONS.reset_text_content, UPDATE_OPTIONS.reset_fabrication_attrs], [false, false, false, false, false]);
  assert.deepEqual([UPDATE_OPTIONS.reset_clearance_overrides, UPDATE_OPTIONS.reset_3d_models, UPDATE_OPTIONS.match_pad_positions], [true, true, true]);
  assert.ok(Object.values(setAllOptions(true)).every(Boolean) && Object.values(setAllOptions(false)).every((v) => !v));
});

const board = {
  parts: [
    { ref: "R1", value: "10k", footprint: "Resistor_SMD:R_0603", placed: true },
    { ref: "R2", value: "4k7", footprint: "Resistor_SMD:R_0805", placed: true },
    { ref: "C1", value: "100n", footprint: "Capacitor_SMD:C_0603", placed: true },
    { ref: "U9", value: "MCU", footprint: "Package_SO:SOIC-8", placed: false },
  ],
} as unknown as BoardState;

test("a choice names the placed footprints it matches: selected, reference, value, library id, all", () => {
  const choice = (mode: "all" | "selected" | "reference" | "value" | "footprint", extra: Partial<{ reference: string; value: string; footprint: string }> = {}) => ({ mode, reference: "", value: "", footprint: "", ...extra });
  assert.deepEqual(matchingFootprints(board, choice("all"), new Set(), null), ["R1", "R2", "C1"], "an unplaced part has no footprint to change");
  assert.deepEqual(matchingFootprints(board, choice("selected"), new Set(["C1"]), "R2"), ["R2", "C1"], "the one the dialog was opened on and the selection");
  assert.deepEqual(matchingFootprints(board, choice("reference", { reference: "r*" }), new Set(), null), ["R1", "R2"]);
  assert.deepEqual(matchingFootprints(board, choice("value", { value: "?k*" }), new Set(), null), ["R2"], "4k7");
  assert.deepEqual(matchingFootprints(board, choice("value", { value: "??K" }), new Set(), null), ["R1"], "10k, the case ignored");
  assert.deepEqual(matchingFootprints(board, choice("footprint", { footprint: "Resistor_SMD:R_0805" }), new Set(), null), ["R2"], "a library id is compared whole");
});

test("a library id has a nickname and an item", () => {
  assert.equal(isLibraryId("Resistor_SMD:R_0603"), true);
  assert.equal(isLibraryId("R_0603"), false);
  assert.equal(isLibraryId(":R"), false);
  assert.equal(isLibraryId("Lib:"), false);
});

test("the message window tells each footprint, and under an updated one what the library copy has otherwise", () => {
  const entry = (reference: string, extra: Record<string, unknown> = {}) => ({ reference, from: "L:A", to: "L:A", found: true, ...extra });
  const entries = [entry("R1", { differs: true, lines: ["Pad 1 size differs.", "Courtyard differs."] }), entry("R2", { differs: false, lines: [] }), entry("R3", { found: false })];
  assert.deepEqual(exchangeReport("update", entries).map((l) => [l.severity, l.text]), [
    ["action", "Updated footprint R1 (L:A): OK"],
    ["info", "    Pad 1 size differs."],
    ["info", "    Courtyard differs."],
    ["info", "Updated footprint R2 (L:A): (no changes)"],
    ["error", "Updated footprint R3 (L:A): *** library footprint not found ***"],
  ]);
  assert.deepEqual(
    exchangeReport("change", [entry("R1", { to: "L:B", differs: true, lines: ["not shown in a change"] })]).map((l) => l.text),
    ["Changed footprint R1 from 'L:A' to 'L:B': OK"],
  );
});
