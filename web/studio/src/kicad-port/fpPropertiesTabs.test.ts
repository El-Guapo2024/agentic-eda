import { test } from "node:test";
import assert from "node:assert/strict";
import { checkFootprintName, footprintExistsMessage, formatMarginOffset, fpidParts, lengthFromText, nextPrivateLayer, parseMarginOffset, privateLayerChoices, settingsForm, settingsOfForm } from "./fpPropertiesTabs";

test("checkFootprintName refuses an empty name and KiCad's invalid characters, in KiCad's words", () => {
  assert.equal(checkFootprintName(""), "Footprint must have a name.");
  assert.equal(checkFootprintName("R/0603"), "Footprint name may not contain '% $ < > 'tab' 'return' 'line feed' \\ \" / :'.");
  assert.equal(checkFootprintName("R:0603"), checkFootprintName("a$b"));
  assert.equal(checkFootprintName("R_0603_1608Metric"), null);
  assert.equal(footprintExistsMessage("R_1", "Mine"), "Footprint 'R_1' already exists in library 'Mine'.");
  assert.deepEqual(fpidParts("Resistor_SMD:R_0603"), { lib: "Resistor_SMD", item: "R_0603" });
  assert.deepEqual(fpidParts("Bare"), { lib: "", item: "Bare" });
});

test("the paste clearance field reads an offset, a percentage or both, as MARGIN_OFFSET_BINDER does", () => {
  assert.deepEqual(parseMarginOffset("", "mm"), { offset: null, ratio: null });
  assert.deepEqual(parseMarginOffset("-0.1mm", "mm"), { offset: -100_000, ratio: null });
  assert.deepEqual(parseMarginOffset("-5%", "mm"), { offset: null, ratio: -0.05 });
  const both = parseMarginOffset("-0.1mm - 5%", "mm")!;
  assert.equal(both.offset, -100_000);
  assert.ok(Math.abs(both.ratio! + 0.05) < 1e-12);
  // a bare number is in the user's units
  assert.deepEqual(parseMarginOffset("-4", "mil"), { offset: -101_600, ratio: null });
  assert.equal(parseMarginOffset("abc", "mm"), null);
  assert.equal(formatMarginOffset(-100_000, -0.05, "mm"), "-0.1 - 5%");
  assert.equal(formatMarginOffset(null, 0.1, "mm"), "10%");
  assert.equal(formatMarginOffset(50_000, 0.025, "mm"), "0.05 + 2.5%");
  assert.equal(formatMarginOffset(0, null, "mm"), "", "an offset of 0 shows as blank");
  assert.equal(lengthFromText("0.5", "mm"), 500_000);
  assert.equal(lengthFromText("20 mil", "mm"), 508_000);
  assert.equal(lengthFromText("x", "mm"), null);
});

test("the tabs come and go back as the footprint's settings, with the dialog's checks", () => {
  const pads = new Set(["1", "2", "3"]);
  const form = settingsForm({ clearance: 200_000, solder_paste_margin: -50_000, solder_paste_margin_ratio: -0.1, net_tie_pad_groups: ["1, 2", ""], jumper_pad_groups: [["1", "3"]], private_layers: ["User.1"] }, "mm");
  assert.equal(form.clearance, "0.2");
  assert.equal(form.solderPaste, "-0.05 - 10%");
  assert.deepEqual(form.netTieGroups, ["1, 2"], "an empty group has no row");
  assert.deepEqual(form.jumperGroups, ["1, 3"]);
  // untouched, the paste field keeps what it was given
  const same = settingsOfForm(form, pads, "mm", { solder_paste_margin: -50_000, solder_paste_margin_ratio: -0.1 });
  assert.ok("settings" in same);
  assert.deepEqual(same.settings, { clearance: 200_000, solder_paste_margin: -50_000, solder_paste_margin_ratio: -0.1, private_layers: ["User.1"], net_tie_pad_groups: ["1, 2"], jumper_pad_groups: [["1", "3"]] });
  // edited: the text is read again; an empty clearance inherits
  const edited = settingsOfForm({ ...form, clearance: "", solderPaste: "-3%", solderPasteTouched: true, jumperGroups: ["3 2,1"], duplicatePadsAreJumpers: true }, pads, "mm", undefined);
  assert.ok("settings" in edited);
  assert.equal(edited.settings.clearance, undefined);
  assert.equal(edited.settings.solder_paste_margin, undefined);
  assert.equal(edited.settings.solder_paste_margin_ratio, -0.03);
  assert.deepEqual(edited.settings.jumper_pad_groups, [["1", "2", "3"]]);
  assert.equal(edited.settings.duplicate_pad_numbers_are_jumpers, true);
  assert.deepEqual(settingsOfForm({ ...form, jumperGroups: ["1", "2, 9"] }, pads, "mm", undefined), { error: "Pad '9' in jumper pad group 2 does not exist in this footprint." });
  assert.deepEqual(settingsOfForm({ ...form, clearance: "-0.1" }, pads, "mm", undefined), { error: "Pad clearance must be at least 0 mm." });
});

test("the Private Layers grid offers the user layers and adds the first one not in it", () => {
  const choices = privateLayerChoices(["F.Cu", "B.Cu", "F.SilkS", "Dwgs.User", "Edge.Cuts", "User.7"]);
  assert.deepEqual(choices, ["Dwgs.User", "User.7", "Cmts.User", "Eco1.User", "Eco2.User", "User.1", "User.2", "User.3", "User.4"]);
  assert.equal(nextPrivateLayer(["Dwgs.User"], choices), "User.7");
  assert.equal(nextPrivateLayer(choices, choices), null);
});
