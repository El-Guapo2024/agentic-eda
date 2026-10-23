// The DRC Rule Editor's pure logic (pcbnew.DRETool.drcRuleEditor; pcbnew/drc/rule_editor/ and common/dialogs/rule_editor_dialog_base.cpp at KiCad
// 8303b2ad): the rule tree (`DIALOG_DRC_RULE_EDITOR::GetDefaultRuleTreeItems`, `buildRuleTreeNode`, `DuplicateRule`, `nodeExists`, `isEnabled`,
// `RULE_EDITOR_DIALOG_BASE::filterRuleTree`, `collectChildRuleNodes`), the condition rows (`DRC_RE_CONDITION_GROUP_PANEL::tokenizeCondition`,
// `ParseCondition`, `BuildCondition`; `DRC_RE_CONDITION_ROW_PANEL::ParseExpression`, `BuildExpression`) and the constraint panels' fields laid over
// their pictures (the `GetFieldPositions` of each `DRC_RE_*_CONSTRAINT_DATA`, macOS offsets). What a rule writes, and how Save checks it, is the
// backend's (`crates/drc/src/rule_editor.rs`), which the dialog asks (`POST /api/rule_editor/check`) and the verb (`edit_drc_rule`) runs again.
// Dependency-free, for the unit tests.

export type PanelType =
  | "minimum_clearance"
  | "creepage_distance"
  | "minimum_connection_width"
  | "copper_to_hole_clearance"
  | "minimum_thermal_relief_spoke_count"
  | "minimum_annular_width"
  | "copper_to_edge_clearance"
  | "courtyard_clearance"
  | "physical_clearance"
  | "minimum_drill_size"
  | "hole_size"
  | "hole_to_hole_distance"
  | "minimum_via_diameter"
  | "via_style"
  | "minimum_text_height_and_thickness"
  | "silk_to_silk_clearance"
  | "silk_to_soldermask_clearance"
  | "minimum_soldermask_sliver"
  | "soldermask_expansion"
  | "solderpaste_expansion"
  | "matched_length_diff_pair"
  | "routing_diff_pair"
  | "routing_width"
  | "maximum_via_count"
  | "absolute_length"
  | "permitted_layers"
  | "allowed_orientation"
  | "vias_under_smd"
  | "custom_rule";

export type ViaType = "any" | "through" | "micro" | "blind" | "buried";
export type RuleSeverity = "undefined" | "ignore" | "warning" | "error" | "exclusion";

/** `eda_drc::rule_editor::Fields`: a panel's values (lengths in mm). */
export interface RuleFields {
  value: number;
  min_via_diameter: number;
  max_via_diameter: number;
  min_via_hole: number;
  max_via_hole: number;
  via_type: ViaType;
  min_text_height: number;
  min_text_thickness: number;
  opt_width: number;
  width_tolerance: number;
  opt_gap: number;
  gap_tolerance: number;
  max_uncoupled: number;
  opt_length: number;
  tolerance: number;
  max_skew: number;
  within_diff_pairs: boolean;
  top_layer: boolean;
  bottom_layer: boolean;
  deg0: boolean;
  deg90: boolean;
  deg180: boolean;
  deg270: boolean;
  all_degrees: boolean;
  through_vias: boolean;
  micro_vias: boolean;
  blind_vias: boolean;
  buried_vias: boolean;
  rule_text: string;
}

/** `eda_drc::rule_editor::Entry` as `GET /api/rule_editor` lists it, with the layer selector's choice for it. */
export interface RuleEntry {
  panel: PanelType;
  fields: RuleFields;
  rule_name: string;
  comment: string;
  rule_condition: string;
  constraint_code: string;
  layers: string[];
  layer_source: string;
  severity: RuleSeverity;
  original_text: string;
  was_edited: boolean;
  generated_rule: string;
  source: number | null;
  layer_token: string;
}

export interface LayerOption {
  token: string;
  label: string;
}

export interface RuleEditorView {
  ok: boolean;
  message?: string;
  entries: RuleEntry[];
  layer_options: Record<string, LayerOption[]>;
  codes: Record<string, string>;
  defaults: RuleFields;
  nets: string[];
  netclasses: string[];
  areas: string[];
}

/** What `Cmd::EditDrcRule`'s Save sends: `eda_drc::rule_editor::RuleEdit`. */
export interface RuleEditPayload {
  panel: PanelType;
  name: string;
  comment: string;
  condition: string;
  layer: string;
  fields: RuleFields;
  value_text?: string | null;
  taken_names?: string[];
  severity?: RuleSeverity | null;
  constraint_code?: string | null;
}

// ------------------------------------------------------------------------------------------------------------------------------------- the tree

export type NodeType = "root" | "category" | "constraint" | "rule";

/** `RULE_TREE_NODE`: a node of the tree. A rule node is either one of the file's (`entry`, its place in the editor's list) or a new one the dialog
 *  holds until it is saved (`draft`, `IsNew`). */
export interface TreeNode {
  id: number;
  name: string;
  type: NodeType;
  parent: number | null;
  panel?: PanelType;
  entry?: number;
  draft?: RuleDraft;
}

/** A new or duplicated rule, before it is saved: the data `buildRuleTreeNode` made (and, for a duplicate, `CopyFrom` copied). */
export interface RuleDraft {
  panel: PanelType;
  rule_name: string;
  comment: string;
  rule_condition: string;
  constraint_code: string;
  layer_token: string;
  severity: RuleSeverity;
  fields: RuleFields;
}

/** `GetDefaultRuleTreeItems` and its `build*RuleTreeNodes`: the fixed nodes, numbered as `buildRuleTreeNodeData` numbers them (from 1, in order). */
export function defaultTree(): TreeNode[] {
  const nodes: TreeNode[] = [];
  let id = 0;
  const add = (name: string, type: NodeType, parent: number | null, panel?: PanelType): number => {
    id += 1;
    nodes.push({ id, name, type, parent, ...(panel ? { panel } : {}) });
    return id;
  };
  const root = add("Design Rules", "root", null);
  const electrical = add("Electrical", "category", root);
  const manufacturability = add("Manufacturability", "category", root);
  const highspeed = add("Highspeed design", "category", root);
  const footprints = add("Footprints", "category", root);

  const clearance = add("Clearance", "category", electrical);
  add("Minimum clearance", "constraint", clearance, "minimum_clearance");
  add("Copper to edge clearance", "constraint", clearance, "copper_to_edge_clearance");
  add("Courtyard clearance", "constraint", clearance, "courtyard_clearance");
  add("Physical clearance", "constraint", clearance, "physical_clearance");
  add("Creepage distance", "constraint", clearance, "creepage_distance");
  add("Minimum connection width", "constraint", electrical, "minimum_connection_width");
  add("Copper to hole clearance", "constraint", electrical, "copper_to_hole_clearance");
  add("Minimum thermal relief spoke count", "constraint", electrical, "minimum_thermal_relief_spoke_count");

  add("Minimum annular width", "constraint", manufacturability, "minimum_annular_width");
  const hole = add("Hole", "category", manufacturability);
  add("Minimum drill size", "constraint", hole, "minimum_drill_size");
  add("Hole to hole distance", "constraint", hole, "hole_to_hole_distance");
  add("Via style", "constraint", hole, "via_style");
  add("Minimum text height and thickness", "constraint", manufacturability, "minimum_text_height_and_thickness");
  add("Silk to silk clearance", "constraint", manufacturability, "silk_to_silk_clearance");
  add("Silk to soldermask clearance", "constraint", manufacturability, "silk_to_soldermask_clearance");
  add("Minimum soldermask sliver", "constraint", manufacturability, "minimum_soldermask_sliver");
  add("Soldermask expansion", "constraint", manufacturability, "soldermask_expansion");
  add("Solderpaste expansion", "constraint", manufacturability, "solderpaste_expansion");

  add("Routing width", "constraint", highspeed, "routing_width");
  add("Maximum via count", "constraint", highspeed, "maximum_via_count");
  add("Routing diff pair", "constraint", highspeed, "routing_diff_pair");
  add("Matched length diff pair", "constraint", highspeed, "matched_length_diff_pair");
  add("Absolute length", "constraint", highspeed, "absolute_length");

  add("Permitted layers", "constraint", footprints, "permitted_layers");
  add("Allowed orientation", "constraint", footprints, "allowed_orientation");
  add("Vias under SMD", "constraint", footprints, "vias_under_smd");

  const custom = add("Custom", "category", root);
  add("Custom rule", "constraint", custom, "custom_rule");
  return nodes;
}

/** The constraint node of a panel. */
export function constraintNode(nodes: readonly TreeNode[], panel: PanelType): TreeNode | undefined {
  return nodes.find((n) => n.type === "constraint" && n.panel === panel);
}

/** The tree with the file's rules (`LoadExistingRules`: each under its panel's node, in the file's order) and then the dialog's new ones. */
export function buildTree(entries: readonly RuleEntry[], drafts: readonly TreeNode[]): TreeNode[] {
  const nodes = defaultTree();
  let id = nodes.length;
  entries.forEach((e, i) => {
    const parent = constraintNode(nodes, e.panel);
    if (!parent) return; // ("No parent found ... skipping"; the backend gives every rule a node.)
    id += 1;
    nodes.push({ id, name: e.rule_name, type: "rule", parent: parent.id, panel: e.panel, entry: i });
  });
  for (const d of drafts) nodes.push(d);
  return nodes;
}

/** `getRuleTreeChildNodes`: a node's children, in the order the nodes were made. */
export function childrenOf(nodes: readonly TreeNode[], parent: number): TreeNode[] {
  return nodes.filter((n) => n.parent === parent);
}

/** `nodeExists`: whether any node of the tree has the name. */
export function nodeExists(nodes: readonly TreeNode[], name: string): boolean {
  return nodes.some((n) => n.name === name);
}

/** `buildRuleTreeNode`'s name: `base 1`, `base 2`, .. the first no node has. */
export function uniqueNodeName(nodes: readonly TreeNode[], base: string): string {
  for (let n = 1; ; n++) {
    const name = `${base} ${n}`;
    if (!nodeExists(nodes, name)) return name;
  }
}

/** `DuplicateRule`: the source's name with a trailing ` <number>` taken off. */
export function duplicateBaseName(name: string): string {
  const i = name.lastIndexOf(" ");
  if (i < 0) return name;
  const suffix = name.slice(i + 1);
  return /^[+-]?\d+$/.test(suffix) ? name.slice(0, i) : name;
}

export type TreeOption = "add_rule" | "duplicate_rule" | "delete_rule" | "move_up" | "move_down";

/** `DIALOG_DRC_RULE_EDITOR::isEnabled`. */
export function isEnabled(node: TreeNode, option: TreeOption): boolean {
  if (option === "add_rule") return node.type === "constraint" || node.type === "rule";
  return node.type === "rule";
}

/** `updateRuleTreeItemMoveOptionState`: a rule moves up unless it is its node's first child, down unless the last. */
export function moveState(nodes: readonly TreeNode[], node: TreeNode): { up: boolean; down: boolean } {
  if (node.type !== "rule" || node.parent === null) return { up: false, down: false };
  const siblings = childrenOf(nodes, node.parent);
  const at = siblings.findIndex((n) => n.id === node.id);
  return { up: at > 0, down: at >= 0 && at < siblings.length - 1 };
}

/** The text an item shows: its name, with ` *` while it is new or has unsaved changes (`appendRuleTreeItem`, `SetModified`). */
export function itemText(node: TreeNode, modified: boolean): string {
  return node.draft || modified ? `${node.name} *` : node.name;
}

/** `filterRuleTree`: the nodes left by the filter -- those whose text has it (case-blind), and every node above one. The root stays. */
export function filterTree(nodes: readonly TreeNode[], filter: string, text: (n: TreeNode) => string): Set<number> {
  const keep = new Set<number>();
  const f = filter.toLowerCase();
  const walk = (n: TreeNode): boolean => {
    let visible = false;
    for (const c of childrenOf(nodes, n.id)) if (walk(c)) visible = true;
    const matches = text(n).toLowerCase().includes(f);
    if (matches || visible) keep.add(n.id);
    return matches || visible;
  };
  const root = nodes.find((n) => n.type === "root");
  if (root) {
    walk(root);
    keep.add(root.id);
  }
  return keep;
}

/** `collectChildRuleNodes`: every rule under a node, depth first in the tree's order. */
export function rulesUnder(nodes: readonly TreeNode[], id: number): TreeNode[] {
  const out: TreeNode[] = [];
  for (const c of childrenOf(nodes, id)) {
    if (c.type === "rule") out.push(c);
    out.push(...rulesUnder(nodes, c.id));
  }
  return out;
}

/** `PANEL_DRC_GROUP_HEADER`'s rows for a category or constraint node: Rule type (the rule's constraint node), Rule name, Comment. */
export function groupRows(nodes: readonly TreeNode[], id: number, comment: (n: TreeNode) => string): { type: string; name: string; comment: string }[] {
  return rulesUnder(nodes, id).map((r) => ({ type: nodes.find((n) => n.id === r.parent)?.name ?? "", name: r.name, comment: comment(r) }));
}

/** `promptUnsavedChanges`'s question. */
export function unsavedChangesMessage(names: readonly string[]): string {
  return `The following rules have unsaved changes:\n\n${names.map((n) => `  • ${n}\n`).join("")}\nDo you want to save your changes?`;
}

// ------------------------------------------------------------------------------------------------------------------------------------- conditions

export type BoolOperator = "and" | "or" | "and_not" | "or_not";
export type ConditionType = "any" | "net" | "netclass" | "within_area" | "custom";

/** One row of `DRC_RE_CONDITION_GROUP_PANEL`: its operator (none on the first row), Object A or B, the condition type and its value. */
export interface ConditionRow {
  op: BoolOperator;
  object: "A" | "B";
  type: ConditionType;
  value: string;
}

export const OPERATOR_LABELS: Record<BoolOperator, string> = { and: "AND", or: "OR", and_not: "AND NOT", or_not: "OR NOT" };
export const CONDITION_LABELS: Record<ConditionType, string> = { any: "Any", net: "Net", netclass: "Netclass", within_area: "Within Area", custom: "Custom Query" };

/** `DRC_RE_CONDITION_GROUP_PANEL::tokenizeCondition`: the expression cut at the `&&` and `||` outside parentheses and quotes; `null` when an operator
 *  is missing where one should be. */
export function tokenizeCondition(expr: string): { op: BoolOperator; expr: string }[] | null {
  const parts: { op: BoolOperator; expr: string }[] = [];
  let remaining = expr;
  let nextOp: BoolOperator = "and";
  let first = true;
  while (remaining.length > 0) {
    remaining = remaining.replace(/^\s+/, "");
    if (remaining.length === 0) break;
    if (!first) {
      if (remaining.startsWith("&&") || remaining.startsWith("||")) {
        const and = remaining.startsWith("&&");
        remaining = remaining.slice(2).replace(/^\s+/, "");
        if (remaining.startsWith("!")) {
          nextOp = and ? "and_not" : "or_not";
          remaining = remaining.slice(1).replace(/^\s+/, "");
        } else {
          nextOp = and ? "and" : "or";
        }
      } else {
        return null;
      }
    }
    let depth = 0;
    let inQuote = false;
    let end = 0;
    for (let i = 0; i < remaining.length; i++) {
      const c = remaining[i];
      if (c === "\\" && i + 1 < remaining.length) {
        i++;
        continue;
      }
      if (c === "'") {
        inQuote = !inQuote;
        continue;
      }
      if (inQuote) continue;
      if (c === "(") depth++;
      if (c === ")") depth--;
      if (depth === 0 && i + 1 < remaining.length) {
        const two = remaining.slice(i, i + 2);
        if (two === "&&" || two === "||") {
          end = i;
          break;
        }
      }
    }
    if (end === 0) end = remaining.length;
    parts.push({ op: nextOp, expr: remaining.slice(0, end).trim() });
    remaining = remaining.slice(end);
    first = false;
  }
  return parts.length > 0 ? parts : null;
}

/** `DRC_RE_CONDITION_ROW_PANEL::ParseExpression`: a row from one part of the condition. `nets` are the board's: a net it does not have selects none
 *  (`NET_SELECTOR::SetSelectedNet`). */
export function parseRow(expr: string, op: BoolOperator, nets: readonly string[]): ConditionRow {
  if (expr === "") return { op, object: "A", type: "any", value: "" };
  let rest = expr;
  let object: "A" | "B" = "A";
  if (rest.startsWith("A.")) rest = rest.slice(2);
  else if (rest.startsWith("B.")) {
    object = "B";
    rest = rest.slice(2);
  }
  let m = /^NetName\s*==\s*'([^']*)'/.exec(rest);
  if (m) return { op, object, type: "net", value: nets.includes(m[1] ?? "") ? (m[1] ?? "") : "" };
  m = /^hasNetclass\('([^']*)'\)/.exec(rest) ?? /^NetClass\s*==\s*'([^']*)'/.exec(rest);
  if (m) return { op, object, type: "netclass", value: m[1] ?? "" };
  m = /^(?:enclosedByArea|intersectsArea)\('([^']*)'\)/.exec(rest);
  if (m) return { op, object, type: "within_area", value: m[1] ?? "" };
  return { op, object, type: "custom", value: expr };
}

/** `DRC_RE_CONDITION_GROUP_PANEL::ParseCondition`: the rows a condition shows -- one Any row for none, one Custom Query row when it does not cut. */
export function parseCondition(expr: string, nets: readonly string[]): ConditionRow[] {
  if (expr === "") return [parseRow("", "and", nets)];
  const parts = tokenizeCondition(expr);
  if (!parts) return [parseRow(expr, "and", nets)];
  return parts.map((p, i) => parseRow(p.expr, i === 0 ? "and" : p.op, nets));
}

/** `DRC_RE_CONDITION_ROW_PANEL::BuildExpression`: a two-object constraint's row says A or B; any other row is about A. */
export function buildRow(row: ConditionRow, twoObjects: boolean): string {
  const prefix = twoObjects ? row.object : "A";
  switch (row.type) {
    case "net":
      return row.value === "" ? "" : `${prefix}.NetName == '${row.value}'`;
    case "netclass":
      return row.value === "" ? "" : `${prefix}.hasNetclass('${row.value}')`;
    case "within_area":
      return row.value === "" ? "" : `${prefix}.intersectsArea('${row.value}')`;
    case "custom":
      return row.value;
    default:
      return "";
  }
}

/** `DRC_RE_CONDITION_GROUP_PANEL::BuildCondition`: the rows joined by their operators, the empty ones left out. */
export function buildCondition(rows: readonly ConditionRow[], twoObjects: boolean): string {
  let result = "";
  for (const row of rows) {
    const e = buildRow(row, twoObjects);
    if (e === "") continue;
    if (result !== "") result += row.op === "and" ? " && " : row.op === "or" ? " || " : row.op === "and_not" ? " && !" : " || !";
    result += e;
  }
  return result;
}

/** `constraintNeedsTwoObjects`. */
export function needsTwoObjects(panel: PanelType): boolean {
  return ["minimum_clearance", "creepage_distance", "copper_to_hole_clearance", "physical_clearance", "hole_to_hole_distance"].includes(panel);
}

// ------------------------------------------------------------------------------------------------------------------------------------- the constraint panels

/** One field over a panel's picture (`DRC_RE_FIELD_POSITION`): its key in `RuleFields` (`value` is typed as text), where it sits, its label. */
export interface OverlayField {
  key: keyof RuleFields | "via_type" | "within_diff_pairs";
  kind: "number" | "check" | "choice";
  x0: number;
  x1: number;
  y: number;
  label?: string;
  labelAt?: "left" | "right";
  /** `±` between this field and the one before it (`plusMinus` of the routing, diff pair and length panels). */
  plusMinus?: boolean;
}

export interface OverlaySpec {
  /** The `BITMAPS` file under `public/icons/{light,dark}/`. */
  bitmap: string;
  fields: OverlayField[];
  /** Integer-only (`IsIntegerOnly`). */
  integer?: boolean;
}

const mmField = (key: OverlayField["key"], x0: number, x1: number, y: number, plusMinus = false): OverlayField => ({ key, kind: "number", x0, x1, y, label: "mm", labelAt: "right", ...(plusMinus ? { plusMinus } : {}) });
const check = (key: OverlayField["key"], x: number, y: number, label: string): OverlayField => ({ key, kind: "check", x0: x, x1: x + 14, y, label, labelAt: "right" });

/** The numeric panels' one field (`drc_re_numeric_constraint_types.h`). */
const NUMERIC: Partial<Record<PanelType, [string, number, number, number, boolean]>> = {
  copper_to_edge_clearance: ["constraint_copper_to_edge_clearance", 208, 248, 160, true],
  copper_to_hole_clearance: ["constraint_copper_to_hole_clearance", 200, 240, 107, true],
  courtyard_clearance: ["constraint_courtyard_clearance", 260, 300, 117, true],
  physical_clearance: ["constraint_clearance", 160, 200, 202, true],
  creepage_distance: ["constraint_creepage_distance", 140, 180, 117, true],
  hole_size: ["constraint_hole_size", 72, 112, 22, true],
  hole_to_hole_distance: ["constraint_hole_to_hole_distance", 102, 142, 35, true],
  maximum_via_count: ["constraint_maximum_via_count", 165, 205, 57, false],
  minimum_annular_width: ["constraint_minimum_annular_width", 209, 249, 67, true],
  minimum_clearance: ["constraint_minimum_clearance", 205, 245, 107, true],
  minimum_connection_width: ["constraint_minimum_connection_width", 230, 270, 104, true],
  minimum_soldermask_sliver: ["constraint_minimum_soldermask_sliver", 202, 242, 290, true],
  minimum_thermal_relief_spoke_count: ["constraint_minimum_thermal_relief_spoke_count", 170, 210, 32, false],
  minimum_drill_size: ["constraint_minimum_drill_size", 209, 249, 107, true],
  minimum_via_diameter: ["constraint_minimum_via_diameter", 186, 226, 105, true],
  silk_to_silk_clearance: ["constraint_silk_to_silk_clearance", 175, 215, 25, true],
  silk_to_soldermask_clearance: ["constraint_silk_to_soldermask_clearance", 235, 275, 60, true],
  soldermask_expansion: ["constraint_soldermask_expansion", 348, 388, 47, true],
  solderpaste_expansion: ["constraint_solderpaste_expansion", 245, 285, 67, true],
};

/** The panel each constraint type opens (`getConstraintPanel`), with its picture and its fields where `GetFieldPositions` puts them. `null` for a
 *  Custom rule, which is a text editor. */
export function overlaySpec(panel: PanelType): OverlaySpec | null {
  const numeric = NUMERIC[panel];
  if (numeric) {
    const [bitmap, x0, x1, y, mm] = numeric;
    const field: OverlayField = mm ? mmField("value", x0, x1, y) : { key: "value", kind: "number", x0, x1, y };
    return { bitmap, fields: [field], ...(panel === "maximum_via_count" || panel === "minimum_thermal_relief_spoke_count" ? { integer: true } : {}) };
  }
  switch (panel) {
    case "via_style":
      return {
        bitmap: "constraint_via_style",
        fields: [
          mmField("min_via_diameter", 155, 195, 10),
          mmField("max_via_diameter", 245, 285, 10),
          mmField("min_via_hole", 195, 235, 190),
          mmField("max_via_hole", 285, 325, 190),
          { key: "via_type", kind: "choice", x0: 420, x1: 510, y: 110, label: "Via type:", labelAt: "left" },
        ],
      };
    case "minimum_text_height_and_thickness":
      return { bitmap: "constraint_minimum_text_height_and_thickness", fields: [mmField("min_text_height", 208, 248, 82), mmField("min_text_thickness", 109, 149, 197)] };
    case "routing_width":
      return { bitmap: "constraint_routing_width", fields: [mmField("opt_width", 30, 70, 15), mmField("width_tolerance", 110, 150, 15, true)] };
    case "routing_diff_pair":
      return {
        bitmap: "constraint_routing_diff_pair",
        fields: [
          mmField("opt_width", 0, 40, 135),
          mmField("width_tolerance", 80, 120, 135, true),
          mmField("opt_gap", 185, 225, 115),
          mmField("gap_tolerance", 265, 305, 115, true),
          mmField("max_uncoupled", 95, 135, 10),
        ],
      };
    case "absolute_length":
      return { bitmap: "constraint_absolute_length_2", fields: [mmField("opt_length", 80, 120, 115), mmField("tolerance", 160, 200, 115, true)] };
    case "matched_length_diff_pair":
      return {
        bitmap: "constraint_matched_length_diff_pair_v2",
        fields: [
          mmField("opt_length", 80, 120, 130),
          mmField("tolerance", 160, 200, 130, true),
          mmField("max_skew", 25, 65, 0),
          // `m_withinDiffPairsCheckbox`: after the skew field's "mm".
          { key: "within_diff_pairs", kind: "check", x0: 96, x1: 110, y: 0, label: "Within diff pairs", labelAt: "right" },
        ],
      };
    case "permitted_layers":
      return { bitmap: "constraint_permitted_layers", fields: [check("top_layer", 150, 40, "Allow top Layer"), check("bottom_layer", 150, 200, "Allow bottom Layer")] };
    case "allowed_orientation":
      return {
        bitmap: "constraint_allowed_orientation",
        fields: [check("deg0", 400, 40, "Allow 0°"), check("deg90", 400, 80, "Allow 90°"), check("deg180", 400, 120, "Allow 180°"), check("deg270", 400, 160, "Allow 270°"), check("all_degrees", 400, 200, "Allow All")],
      };
    case "vias_under_smd":
      return {
        bitmap: "constraint_vias_under_smd",
        fields: [
          check("through_vias", 400, 40, "Disallow Through Via"),
          check("micro_vias", 400, 80, "Disallow Micro Via"),
          check("blind_vias", 400, 120, "Disallow Blind Via"),
          check("buried_vias", 400, 160, "Disallow Buried Via"),
        ],
      };
    default:
      return null;
  }
}

/** The allowed-orientation panel's boxes (`onIndividualCheckbox`, `onAllDegreesCheckbox`): Allow All follows the four, and sets them all. */
export function toggleOrientation(f: RuleFields, key: keyof RuleFields, on: boolean): RuleFields {
  if (key === "all_degrees") return { ...f, all_degrees: on, deg0: on, deg90: on, deg180: on, deg270: on };
  const next = { ...f, [key]: on } as RuleFields;
  return { ...next, all_degrees: next.deg0 && next.deg90 && next.deg180 && next.deg270 };
}

/** A length as the panel's unit binder shows it (mm): up to four decimals, the trailing zeros taken off. */
export function showMm(v: number): string {
  if (!Number.isFinite(v)) return "";
  const s = v.toFixed(4);
  return s.includes(".") ? s.replace(/0+$/, "").replace(/\.$/, "") : s;
}

/** A field as typed back to a number (`UNIT_BINDER::GetDoubleValue` in mm): a comma reads as a decimal point; what is no number is 0. */
export function readMm(text: string): number {
  const v = Number.parseFloat(text.trim().replace(",", "."));
  return Number.isFinite(v) ? v : 0;
}

/** `DRC_RE_CUSTOM_RULE_PANEL::TransferDataToWindow`'s text for a rule that has none. */
export const CUSTOM_RULE_PLACEHOLDER = "   (constraint clearance (min 0.2mm))";
