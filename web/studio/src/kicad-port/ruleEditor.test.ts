import { test } from "node:test";
import assert from "node:assert/strict";
import {
  buildCondition,
  buildTree,
  childrenOf,
  constraintNode,
  defaultTree,
  duplicateBaseName,
  filterTree,
  groupRows,
  isEnabled,
  moveState,
  overlaySpec,
  parseCondition,
  readMm,
  showMm,
  tokenizeCondition,
  toggleOrientation,
  uniqueNodeName,
  unsavedChangesMessage,
  type PanelType,
  type RuleEntry,
  type RuleFields,
} from "./ruleEditor";

const fields = (): RuleFields => ({
  value: 0,
  min_via_diameter: 0,
  max_via_diameter: 0,
  min_via_hole: 0,
  max_via_hole: 0,
  via_type: "any",
  min_text_height: 0,
  min_text_thickness: 0,
  opt_width: 0,
  width_tolerance: 0,
  opt_gap: 0,
  gap_tolerance: 0,
  max_uncoupled: 0,
  opt_length: 0,
  tolerance: 0,
  max_skew: 0,
  within_diff_pairs: false,
  top_layer: false,
  bottom_layer: false,
  deg0: false,
  deg90: false,
  deg180: false,
  deg270: false,
  all_degrees: false,
  through_vias: true,
  micro_vias: true,
  blind_vias: true,
  buried_vias: true,
  rule_text: "",
});

const entry = (panel: PanelType, name: string, comment = ""): RuleEntry => ({
  panel,
  fields: fields(),
  rule_name: name,
  comment,
  rule_condition: "",
  constraint_code: "",
  layers: [],
  layer_source: "",
  severity: "undefined",
  original_text: "",
  was_edited: false,
  generated_rule: "",
  source: 0,
  layer_token: "",
});

test("the tree is KiCad's: five categories under Design Rules, a node per panel, Custom last", () => {
  const t = defaultTree();
  assert.equal(t[0]?.name, "Design Rules");
  assert.deepEqual(childrenOf(t, 1).map((n) => n.name), ["Electrical", "Manufacturability", "Highspeed design", "Footprints", "Custom"]);
  const electrical = t.find((n) => n.name === "Electrical")!;
  assert.deepEqual(childrenOf(t, electrical.id).map((n) => n.name), ["Clearance", "Minimum connection width", "Copper to hole clearance", "Minimum thermal relief spoke count"]);
  assert.equal(t.filter((n) => n.type === "constraint").length, 27);
  // Two panels have no node: their rules come as other panels' (the backend's fallback).
  assert.equal(constraintNode(t, "hole_size"), undefined);
  assert.equal(constraintNode(t, "minimum_via_diameter"), undefined);
  assert.equal(constraintNode(t, "via_style")?.name, "Via style");
});

test("the file's rules hang under their panels, in the file's order, and new ones after them", () => {
  const entries = [entry("minimum_clearance", "a"), entry("routing_width", "w"), entry("minimum_clearance", "b")];
  const t = buildTree(entries, []);
  const mc = constraintNode(t, "minimum_clearance")!;
  assert.deepEqual(childrenOf(t, mc.id).map((n) => [n.name, n.entry]), [["a", 0], ["b", 2]]);
  const draft = { id: 999, name: "Minimum clearance 1", type: "rule" as const, parent: mc.id, panel: "minimum_clearance" as const, draft: { panel: "minimum_clearance" as const, rule_name: "Minimum clearance 1", comment: "", rule_condition: "", constraint_code: "clearance", layer_token: "", severity: "undefined" as const, fields: fields() } };
  const t2 = buildTree(entries, [draft]);
  assert.deepEqual(childrenOf(t2, mc.id).map((n) => n.name), ["a", "b", "Minimum clearance 1"]);
  assert.deepEqual(moveState(t2, childrenOf(t2, mc.id)[0]!), { up: false, down: true });
  assert.deepEqual(moveState(t2, childrenOf(t2, mc.id)[1]!), { up: true, down: true });
  assert.deepEqual(moveState(t2, mc), { up: false, down: false });
  // The group header lists every rule under a category, with its type.
  const electrical = t2.find((n) => n.name === "Electrical")!;
  assert.deepEqual(groupRows(t2, electrical.id, (n) => (n.entry === 0 ? "first" : "")), [
    { type: "Minimum clearance", name: "a", comment: "first" },
    { type: "Minimum clearance", name: "b", comment: "" },
    { type: "Minimum clearance", name: "Minimum clearance 1", comment: "" },
  ]);
});

test("new and duplicated rules are named as buildRuleTreeNode names them", () => {
  const t = buildTree([entry("minimum_clearance", "Minimum clearance 1"), entry("minimum_clearance", "HV 2")], []);
  assert.equal(uniqueNodeName(t, "Minimum clearance"), "Minimum clearance 2");
  assert.equal(uniqueNodeName(t, duplicateBaseName("HV 2")), "HV 1");
  assert.equal(duplicateBaseName("rule"), "rule");
  assert.equal(duplicateBaseName("rule two"), "rule two");
});

test("the context menu offers what isEnabled allows", () => {
  const t = buildTree([entry("via_style", "v")], []);
  const rule = t.find((n) => n.type === "rule")!;
  const node = constraintNode(t, "via_style")!;
  const category = t.find((n) => n.name === "Hole")!;
  assert.deepEqual([isEnabled(rule, "add_rule"), isEnabled(rule, "duplicate_rule"), isEnabled(rule, "delete_rule"), isEnabled(rule, "move_up")], [true, true, true, true]);
  assert.deepEqual([isEnabled(node, "add_rule"), isEnabled(node, "duplicate_rule"), isEnabled(node, "delete_rule")], [true, false, false]);
  assert.deepEqual([isEnabled(category, "add_rule"), isEnabled(category, "delete_rule")], [false, false]);
});

test("the filter keeps the matches and every node above them", () => {
  const t = buildTree([entry("routing_width", "HS width")], []);
  const keep = filterTree(t, "hs W", (n) => n.name);
  assert.deepEqual([...keep].map((id) => t.find((n) => n.id === id)!.name).sort(), ["Design Rules", "HS width", "Highspeed design", "Routing width"].sort());
  assert.equal(filterTree(t, "", (n) => n.name).size, t.length);
});

test("a condition is cut into rows at its top-level && and ||", () => {
  assert.deepEqual(tokenizeCondition("A.NetName == 'GND' && (B.Type == 'Via' || B.Type == 'Pad') || !A.isPlated()"), [
    { op: "and", expr: "A.NetName == 'GND'" },
    { op: "and", expr: "(B.Type == 'Via' || B.Type == 'Pad')" },
    { op: "or_not", expr: "A.isPlated()" },
  ]);
  assert.deepEqual(tokenizeCondition("A.Name == 'a && b' && A.x"), [
    { op: "and", expr: "A.Name == 'a && b'" },
    { op: "and", expr: "A.x" },
  ]);
  assert.equal(tokenizeCondition(""), null);
});

test("a row reads Net, Netclass and Within Area, and anything else as a Custom Query", () => {
  const nets = ["GND", "/VCC"];
  const rows = parseCondition("A.NetName == 'GND' && B.hasNetclass('HV') || A.intersectsArea('Keep') && B.NetClass == 'Power' && A.Type == 'Pad'", nets);
  assert.deepEqual(rows.map((r) => [r.op, r.object, r.type, r.value]), [
    ["and", "A", "net", "GND"],
    ["and", "B", "netclass", "HV"],
    ["or", "A", "within_area", "Keep"],
    ["and", "B", "netclass", "Power"],
    ["and", "A", "custom", "A.Type == 'Pad'"],
  ]);
  // A net the board does not have selects none, and the row then writes nothing.
  const unknown = parseCondition("A.NetName == 'NOPE'", nets);
  assert.deepEqual([unknown[0]?.type, unknown[0]?.value], ["net", ""]);
  assert.equal(buildCondition(unknown, true), "");
  // Built back: hasNetclass and intersectsArea, B only for a two-object constraint.
  assert.equal(buildCondition(rows, true), "A.NetName == 'GND' && B.hasNetclass('HV') || A.intersectsArea('Keep') && B.hasNetclass('Power') && A.Type == 'Pad'");
  assert.equal(buildCondition(rows, false), "A.NetName == 'GND' && A.hasNetclass('HV') || A.intersectsArea('Keep') && A.hasNetclass('Power') && A.Type == 'Pad'");
  assert.deepEqual(parseCondition("", nets), [{ op: "and", object: "A", type: "any", value: "" }]);
  assert.equal(buildCondition([{ op: "and", object: "A", type: "any", value: "" }, { op: "and_not", object: "A", type: "custom", value: "A.isPlated()" }], false), "A.isPlated()");
  assert.equal(buildCondition([{ op: "and", object: "A", type: "custom", value: "A.x" }, { op: "or_not", object: "A", type: "custom", value: "A.y" }], false), "A.x || !A.y");
});

test("each panel's fields sit where GetFieldPositions puts them", () => {
  assert.deepEqual(overlaySpec("minimum_clearance"), { bitmap: "constraint_minimum_clearance", fields: [{ key: "value", kind: "number", x0: 205, x1: 245, y: 107, label: "mm", labelAt: "right" }] });
  assert.equal(overlaySpec("maximum_via_count")?.integer, true);
  assert.equal(overlaySpec("maximum_via_count")?.fields[0]?.label, undefined);
  assert.equal(overlaySpec("via_style")?.fields.length, 5);
  assert.deepEqual(overlaySpec("vias_under_smd")?.fields.map((f) => f.label), ["Disallow Through Via", "Disallow Micro Via", "Disallow Blind Via", "Disallow Buried Via"]);
  assert.equal(overlaySpec("custom_rule"), null);
  const every: PanelType[] = ["minimum_clearance", "creepage_distance", "minimum_connection_width", "copper_to_hole_clearance", "minimum_thermal_relief_spoke_count", "minimum_annular_width", "copper_to_edge_clearance", "courtyard_clearance", "physical_clearance", "minimum_drill_size", "hole_to_hole_distance", "via_style", "minimum_text_height_and_thickness", "silk_to_silk_clearance", "silk_to_soldermask_clearance", "minimum_soldermask_sliver", "soldermask_expansion", "solderpaste_expansion", "matched_length_diff_pair", "routing_diff_pair", "routing_width", "maximum_via_count", "absolute_length", "permitted_layers", "allowed_orientation", "vias_under_smd"];
  for (const p of every) assert.ok(overlaySpec(p)?.bitmap.startsWith("constraint_"), p);
});

test("Allow All follows the four angles and sets them", () => {
  let f = toggleOrientation(fields(), "all_degrees", true);
  assert.deepEqual([f.deg0, f.deg90, f.deg180, f.deg270, f.all_degrees], [true, true, true, true, true]);
  f = toggleOrientation(f, "deg90", false);
  assert.deepEqual([f.deg90, f.all_degrees], [false, false]);
  f = toggleOrientation(f, "deg90", true);
  assert.equal(f.all_degrees, true);
});

test("lengths show and read as the unit binder's mm", () => {
  assert.equal(showMm(0.25), "0.25");
  assert.equal(showMm(2147.483647), "2147.4836");
  assert.equal(showMm(3), "3");
  assert.equal(readMm("0,2"), 0.2);
  assert.equal(readMm("abc"), 0);
  assert.equal(unsavedChangesMessage(["a", "b"]), "The following rules have unsaved changes:\n\n  • a\n  • b\n\nDo you want to save your changes?");
});
