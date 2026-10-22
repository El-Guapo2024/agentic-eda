import { test } from "node:test";
import assert from "node:assert/strict";
import type { Group, Zone } from "../api/types";
import {
  anchorChoices,
  chooseMode,
  copiedMessage,
  DEFAULT_REPEAT_OPTIONS,
  generateCommand,
  groupItemsAvailable,
  initialGenerateTab,
  isPlacementRuleArea,
  notFoundLabel,
  placementFormOf,
  placementOf,
  placementOptions,
  referenceAreaOf,
  repeatCommand,
  statusOf,
  targetRows,
  type GenerateRow,
} from "./multichannel";

const zone = (id: string, extra: Partial<Zone> = {}): Zone => ({ id, net: "", layer: "F.Cu", outline: [], is_rule_area: true, ...extra }) as unknown as Zone;
const placed = (id: string, source = "/ch1/"): Zone => zone(id, { placement: { enabled: true, source_type: "sheetname", source } });
const row = (kind: GenerateRow["kind"], key: string, generate = false): GenerateRow => ({ kind, key, file: kind === "sheet" ? "channel.kicad_sch" : "", components: [], generate });

test("the Placement tab opens on the radio of the area's source type when placement is on, and on No placement when it is off", () => {
  const choices = { sheets: ["", "/ch1/", "/ch2/"], classes: ["Passives"], groups: ["A"] };
  const on = placementFormOf({ enabled: true, source_type: "component_class", source: "Passives" }, choices);
  assert.equal(on.mode, "component_class");
  assert.equal(on.component_class, "Passives");
  assert.equal(on.sheet, "", "the combo boxes of the other types start on their first entry");
  const off = placementFormOf({ enabled: false, source_type: "sheetname", source: "/ch2/" }, choices);
  assert.equal(off.mode, "none");
  assert.equal(off.sheet, "/ch2/");
  // an area that has nothing set
  const none = placementFormOf(undefined, choices);
  assert.deepEqual([none.mode, none.sheet, none.component_class, none.group, none.last], ["none", "", "Passives", "A", "sheetname"]);
});

test("a source the board no longer has is put first as not found and kept unless another is chosen", () => {
  const choices = { sheets: ["/ch1/"], classes: [], groups: [] };
  const form = placementFormOf({ enabled: true, source_type: "sheetname", source: "/gone/" }, choices);
  assert.equal(form.sheet, "/gone/");
  const options = placementOptions(form, choices, "sheetname");
  assert.deepEqual(options, [{ value: "/gone/", label: notFoundLabel("/gone/") }, { value: "/ch1/", label: "/ch1/" }]);
  assert.deepEqual(placementOf(form), { enabled: true, source_type: "sheetname", source: "/gone/" });
  assert.deepEqual(placementOf({ ...form, sheet: "/ch1/" }), { enabled: true, source_type: "sheetname", source: "/ch1/" });
  // the empty sheet reads as "(no sheet)"
  assert.equal(placementOptions(placementFormOf(undefined, { sheets: [""], classes: [], groups: [] }), { sheets: [""], classes: [], groups: [] }, "sheetname")[0]!.label, "(no sheet)");
});

test("turning placement off keeps the last source type chosen and what its combo box shows (TransferDataFromWindow)", () => {
  const choices = { sheets: ["/ch1/"], classes: ["Passives"], groups: ["A"] };
  let form = placementFormOf(undefined, choices);
  form = chooseMode(form, "group");
  form = { ...form, group: "A" };
  assert.deepEqual(placementOf(form), { enabled: true, source_type: "group", source: "A" });
  form = chooseMode(form, "none");
  assert.deepEqual(placementOf(form), { enabled: false, source_type: "group", source: "A" });
});

test("the Generate dialog opens on the sheets, but on the classes (else the groups) when there is a single sheet", () => {
  assert.equal(initialGenerateTab([row("sheet", "/a/"), row("sheet", "/b/"), row("component_class", "X")]), "sheet");
  assert.equal(initialGenerateTab([row("sheet", "/a/"), row("component_class", "X")]), "component_class");
  assert.equal(initialGenerateTab([row("sheet", "/a/"), row("group", "G")]), "group");
  assert.equal(initialGenerateTab([row("sheet", "/a/")]), "sheet");
});

test("the Generate command lists the rows ticked, and never groups from the Groups tab", () => {
  const rows = [row("sheet", "/a/", true), row("sheet", "/b/"), row("group", "G", true)];
  const cmd = generateCommand(rows, true, true, "sheet");
  assert.deepEqual(cmd, { op: "multichannel", verb: "generate_rule_areas", areas: [{ kind: "sheet", key: "/a/" }, { kind: "group", key: "G" }], replace_existing: true, group_items: true });
  assert.equal((generateCommand(rows, false, true, "group") as { group_items: boolean }).group_items, false);
  assert.equal(groupItemsAvailable("group"), false);
  assert.equal(generateCommand([row("sheet", "/a/")], false, false, "sheet"), null, "no row ticked makes nothing");
});

test("the reference area of Repeat Layout is the one placement rule area selected, or in a selected group", () => {
  const zones = [placed("z1"), placed("z2", "/ch2/"), zone("keep"), zone("off", { placement: { enabled: false, source_type: "sheetname", source: "" } })];
  const groups: Group[] = [{ id: "g1", name: "", member_ids: ["z2", "R1"] } as Group, { id: "g2", name: "", member_ids: ["z1", "z2"] } as Group];
  assert.equal(referenceAreaOf(["z1"], zones, groups), "z1");
  assert.equal(referenceAreaOf(["g1"], zones, groups), "z2");
  assert.equal(referenceAreaOf(["z1", "g1"], zones, groups), null, "two of them are no reference");
  assert.equal(referenceAreaOf(["keep"], zones, groups), null, "a keepout is no placement area");
  assert.equal(referenceAreaOf(["off"], zones, groups), null);
  assert.equal(referenceAreaOf([], zones, groups), null);
  assert.equal(referenceAreaOf(["g2"], zones, groups), null);
  assert.equal(isPlacementRuleArea(zones[0]), true);
  assert.equal(isPlacementRuleArea(zones[2]), false);
});

test("the Repeat Layout rows start ticked for the targets that match, and the command copies to those only", () => {
  const rows = targetRows([
    { id: "a", name: "A", ok: true, message: "OK", reasons: [], components: [] },
    { id: "b", name: "B", ok: false, message: "Component count mismatch", reasons: ["Component count mismatch"], components: [] },
  ]);
  assert.deepEqual(rows.map((r) => r.copy), [true, false]);
  assert.deepEqual([statusOf(rows[0]!), statusOf(rows[1]!)], ["OK", "Component count mismatch"]);
  const cmd = repeatCommand("ref", rows, { ...DEFAULT_REPEAT_OPTIONS, anchor: "" });
  assert.deepEqual(cmd, { op: "multichannel", verb: "repeat_layout", reference: "ref", targets: ["a"], options: { ...DEFAULT_REPEAT_OPTIONS, anchor: null } });
  // a target that does not match cannot be copied to even if it is ticked
  assert.equal(repeatCommand("ref", rows.map((r) => ({ ...r, copy: !r.ok })), DEFAULT_REPEAT_OPTIONS), null);
  assert.equal(copiedMessage(3), "Copied to 3 Rule Areas.");
  assert.deepEqual(anchorChoices(["U2", "C1", "R1"]), ["", "C1", "R1", "U2"]);
  assert.deepEqual(DEFAULT_REPEAT_OPTIONS, { copy_placement: true, copy_routing: true, connected_routing_only: false, copy_other_items: true, group_items: false, include_locked_items: true, anchor: null });
});
