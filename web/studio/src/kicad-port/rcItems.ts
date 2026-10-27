// The Checker windows' lists -- what the DRC and ERC dialogs show and how a marker is acted on -- as pure functions.
//
// Ported from:
//   common/rc_item.cpp                 RC_TREE_MODEL: which markers are listed (the provider's severity filter), the prefix of a marker's line
//                                      ("Error: ", "Excluded warning: "), Next / Previous Marker over the listed ones
//   pcbnew/dialogs/dialog_drc.cpp      OnDRCItemRClick (the marker menu), OnSeverity (the Show boxes), ExcludeMarker, updateDisplayedCounts
//   eeschema/dialogs/dialog_erc.cpp    OnERCItemRClick, OnSeverity, ExcludeMarker
//
// The dialogs (components/DrcDialog.tsx, ErcDialog.tsx) and the canvases' marker menus render what these return and run the commands they
// build; nothing here knows React or the store.
//
//   common/rc_item.cpp                RC_TREE_MODEL::DeleteCurrentItem / DeleteItems (Delete Marker, Delete All Markers)
//   pcbnew/tools/board_inspection_tool.cpp  InspectDRCErrorMenuText     pcbnew/tools/drc_tool.cpp  FixDRCErrorMenuText / FixDRCError
//   eeschema/tools/sch_inspection_tool.cpp  InspectERCErrorMenuText     eeschema/tools/sch_edit_tool.cpp  FixERCErrorMenuText / FixERCError
//
// What differs from KiCad, and why:
//   - "Exclude all violations of rule '...'" is per violated *rule* in KiCad (`DRC_ITEM::GetViolatingRule`, a custom rule that is not implicit; DRC only).
//     kicad-cli's report does not carry the rule, but it names it in the violation's text -- `DRC_CONSTRAINT::GetName` is "rule '<name>'" for a custom
//     rule and the bare name of an implicit one -- so the rule is read from there ([`violatingRule`]). The menu also offers "all '<check>' violations"
//     -- every violation of the check -- on the board and on the schematic, which KiCad's does not.
//   - The ERC "Edit Netclasses..." fix opens Schematic Setup > Net Classes, a page this app does not have; its entry is left out.
import type { DrcExclusionSpec, DrcReport, DrcViolation, ErcReport, ErcViolation, RuleSeverity } from "../api/types";
import { nextMarkerIndex, prevMarkerIndex } from "./checkerNav";
import { severitiesToSend, severityOf, type SeverityItem } from "./boardSetupRules";

// ------------------------------------------------------------------------------------------------------------ what is listed

/** The dialog's "Show" boxes: `getSeverities()` -- `RPT_SEVERITY_ERROR`, `_WARNING` and `_EXCLUSION` -- as booleans. */
export interface RcFilter {
  errors: boolean;
  warnings: boolean;
  exclusions: boolean;
}

export const SHOW_ALL: RcFilter = { errors: true, warnings: true, exclusions: true };

/** `OnSeverity` on the "All" box: errors stay on, warnings and exclusions follow the box. Unchecking it leaves the errors only. */
export function setShowAll(checked: boolean): RcFilter {
  return { errors: true, warnings: checked, exclusions: checked };
}

/** Whether the "All" box is drawn checked: every severity is on. */
export function allShown(f: RcFilter): boolean {
  return f.errors && f.warnings && f.exclusions;
}

/** A marker as the list files it (`MARKER_BASE::GetSeverity`): a waived marker is an exclusion, whatever the check's severity is. */
export type RcKind = "error" | "warning" | "exclusion";

/** `entry.excluded` for a DRC violation; an ERC finding is `severity: "excluded"` (the schematic report folds the two together). */
export function rcKind(e: { severity: string; excluded?: boolean }): RcKind {
  if (e.excluded || e.severity === "excluded") return "exclusion";
  return e.severity === "warning" ? "warning" : "error";
}

/** `RC_ITEMS_PROVIDER::SetSeverities` + `GetItem`: whether a marker of this kind is listed under the Show boxes. */
export function isListed(kind: RcKind, f: RcFilter): boolean {
  return kind === "error" ? f.errors : kind === "warning" ? f.warnings : f.exclusions;
}

/**
 * The indexes of the entries the list shows: every error, then the warnings, then the exclusions, each kind in report order (r2 D-E3:
 * `SHEETLIST_ERC_ITEMS_PROVIDER::SetSeverities` (erc_settings.cpp) and `DRC_ITEMS_PROVIDER::SetSeverities` (drc_item.cpp): "Sort markers so
 * that errors appear before warnings", `std::stable_sort( .., a->GetSeverity() > b->GetSeverity() )`, ERROR 0x20 > WARNING 0x10 > EXCLUSION 0x04,
 * an excluded marker's `GetSeverity` being the exclusion). Next / Previous Marker step in this order too.
 *
 * The sort runs when the provider refilters (`RC_TREE_MODEL::Update` -> `rebuildTree` -> `SetSeverities`). The ERC dialog does that on every
 * exclusion change (`OnERCItemRClick`'s ID_ADD_EXCLUSION / ID_REMOVE_EXCLUSION and `DIALOG_ERC::ExcludeMarker` call `Update`), so an ERC row that is
 * waived moves at once. The DRC dialog waives or restores one marker with `ValueChanged( node )` (or `DeleteCurrentItem` when exclusions are not
 * shown), with no refilter, so the row stays where it was until the next refilter: such a row carries `listed_as`, the kind it was sorted as
 * ([`patchDrcExcluded`] with `hold`, cleared by [`refilterDrc`]).
 */
export function listedIndexes<T extends { severity: string; excluded?: boolean; listed_as?: RcKind }>(entries: readonly T[], f: RcFilter): number[] {
  const rank = (k: RcKind) => (k === "error" ? 0 : k === "warning" ? 1 : 2);
  const out: Array<[number, number]> = [];
  entries.forEach((e, i) => {
    const kind = rcKind(e);
    if (isListed(kind, f)) out.push([rank(e.listed_as ?? kind), i]);
  });
  return out.sort((a, b) => a[0] - b[0] || a[1] - b[1]).map(([, i]) => i);
}

export interface RcCounts {
  errors: number;
  warnings: number;
  exclusions: number;
}

/** `updateDisplayedCounts`: how many markers of each kind the reports hold, whatever the Show boxes say. */
export function countKinds(...lists: ReadonlyArray<ReadonlyArray<{ severity: string; excluded?: boolean }>>): RcCounts {
  const c: RcCounts = { errors: 0, warnings: 0, exclusions: 0 };
  for (const list of lists) {
    for (const e of list) {
      const kind = rcKind(e);
      if (kind === "error") c.errors++;
      else if (kind === "warning") c.warnings++;
      else c.exclusions++;
    }
  }
  return c;
}

/**
 * `RC_TREE_MODEL::GetValue` for a marker's own line: what comes before the message. An excluded marker says which severity it would have had
 * (`GetSeverity( code )`, the check's current setting).
 */
export function markerPrefix(kind: RcKind, settingSeverity: RuleSeverity | string | undefined): string {
  if (kind === "exclusion") return settingSeverity === "warning" ? "Excluded warning: " : "Excluded error: ";
  return kind === "warning" ? "Warning: " : "Error: ";
}

// ------------------------------------------------------------------------------------------------------------ Next / Previous Marker

/**
 * `RC_TREE_MODEL::NextMarker` / `PrevMarker` over the listed markers: the one after (before) the current one, the first (last) when none is
 * selected, nothing at the end. `listed` is [`listedIndexes`]; `current` an index into the report (null for none, or a marker the list does not
 * show). Returns the report index to select, or null to leave the selection where it is.
 */
export function stepListed(listed: readonly number[], current: number | null, dir: "next" | "prev"): number | null {
  const at = current === null ? -1 : listed.indexOf(current);
  const pos = at === -1 ? null : at;
  const to = (dir === "next" ? nextMarkerIndex : prevMarkerIndex)(listed.length, pos);
  return to === null ? null : (listed[to] ?? null);
}

/**
 * Where the selection goes when the current row leaves the list (`RC_TREE_MODEL::DeleteCurrentItem`, which an exclusion triggers while the Show boxes
 * leave exclusions out): the row that followed it, or the one before when it was the last; null when it was the only one. `listed` is the list as it was.
 */
export function selectionAfterRowGone(listed: readonly number[], gone: number): number | null {
  const at = listed.indexOf(gone);
  if (at === -1) return null;
  return listed[at + 1] ?? listed[at - 1] ?? null;
}

// ------------------------------------------------------------------------------------------------------------ the marker menu

export type RcMenuId =
  | "remove_exclusion"
  | "edit_comment"
  | "remove_exclusion_type"
  | "remove_exclusion_rule"
  | "exclude"
  | "exclude_comment"
  | "exclude_type"
  | "exclude_rule"
  | "inspect"
  | "fix"
  | "connection_grid"
  | "severity_error"
  | "severity_warning"
  | "severity_ignore"
  | "edit_severities"
  | "show_in_dialog";

export interface RcMenuEntry {
  id: RcMenuId | "separator";
  label: string;
  /** KiCad's status-bar help for the entry; shown as a tooltip. */
  hint?: string;
  disabled?: boolean;
}

export interface RcMenuContext {
  domain: "drc" | "erc";
  /** `RC_ITEM::GetErrorText( true )`: the check's title ("Clearance violation"). */
  title: string;
  /** The marker is waived now. */
  excluded: boolean;
  /** The check's severity setting now; decides the "appropriate" list name and which severity change is offered. */
  severity: RuleSeverity;
  /** `ERCE_PIN_TO_PIN_WARNING` / `_ERROR`: their severities are the pin conflicts map's -- only Ignore is a choice here. */
  pinMap?: boolean;
  /** The menu is the canvas marker's, which also offers to show the marker in the dialog. */
  onCanvas?: boolean;
  /** An exclusion can carry a comment (`m_DrcExclusionComments`, `m_ErcExclusionComments`): both menus offer "Exclude with comment..." and "Edit exclusion comment...". Default true. */
  comments?: boolean;
  /** DRC only: the name of the custom rule the violation broke ([`violatingRule`]); null for a violation of an implicit rule (Board Setup's, a netclass's ...), which has no "all violations of rule" entry. */
  rule?: string | null;
  /** The Inspect entry the marker's check has ([`drcInspectEntry`], [`ercInspectEntry`]), and its Fix entry; none for most checks. */
  inspect?: MarkerEntry | null;
  fix?: MarkerEntry | null;
  /** ERC only: `ERCE_ENDPOINT_OFF_GRID` also offers "Edit connection grid spacing...". */
  connectionGrid?: boolean;
}

/**
 * The menu of one marker. The dialogs' own is `OnDRCItemRClick` / `OnERCItemRClick`; the canvas marker's is the same plus "Show in the dialog"
 * (`DRC_TOOL::CrossProbe`: the dialog comes up with the marker's row selected).
 */
export function rcMenu(c: RcMenuContext): RcMenuEntry[] {
  const listName = c.severity === "error" ? "errors" : c.severity === "warning" ? "warnings" : "appropriate";
  const setup = c.domain === "drc" ? "Board Setup" : "Schematic Setup";
  const out: RcMenuEntry[] = [];
  const comments = c.comments ?? true;
  if (c.excluded) {
    out.push({ id: "remove_exclusion", label: "Remove exclusion for this violation", hint: `It will be placed back in the ${listName} list` });
    if (comments) out.push({ id: "edit_comment", label: "Edit exclusion comment..." });
    if (c.rule) out.push({ id: "remove_exclusion_rule", label: `Remove all exclusions for violations of rule '${c.rule}'`, hint: `They will be placed back in the ${listName} list` });
    out.push({ id: "remove_exclusion_type", label: `Remove all exclusions for '${c.title}' violations`, hint: `They will be placed back in the ${listName} list` });
  } else {
    out.push({ id: "exclude", label: "Exclude this violation", hint: `It will be excluded from the ${listName} list` });
    if (comments) out.push({ id: "exclude_comment", label: "Exclude with comment...", hint: `It will be excluded from the ${listName} list` });
    if (c.rule) out.push({ id: "exclude_rule", label: `Exclude all violations of rule '${c.rule}'...`, hint: `They will be excluded from the ${listName} list` });
    out.push({ id: "exclude_type", label: `Exclude all '${c.title}' violations`, hint: `They will be excluded from the ${listName} list` });
  }
  out.push({ id: "separator", label: "" });
  // `InspectDRCErrorMenuText` / `FixDRCErrorMenuText`: the entries a check has to look into the violation or to put it right, then a separator of their own.
  if (c.inspect || c.fix) {
    if (c.inspect) out.push({ id: "inspect", label: c.inspect.label });
    if (c.fix) out.push({ id: "fix", label: c.fix.label });
    out.push({ id: "separator", label: "" });
  }
  if (!c.pinMap) {
    if (c.severity === "warning") out.push({ id: "severity_error", label: `Change severity to Error for all '${c.title}' violations`, hint: `Violation severities can also be edited in ${setup}` });
    else out.push({ id: "severity_warning", label: `Change severity to Warning for all '${c.title}' violations`, hint: `Violation severities can also be edited in ${setup}` });
  }
  out.push({ id: "severity_ignore", label: `Ignore all '${c.title}' violations`, hint: "Violations will not be checked or reported" });
  out.push({ id: "separator", label: "" });
  out.push(
    c.pinMap
      ? { id: "edit_severities", label: "Edit pin-to-pin conflict map...", hint: `Open the ${setup} dialog` }
      : { id: "edit_severities", label: "Edit violation severities...", hint: `Open the ${setup} dialog` }
  );
  // `ID_EDIT_CONNECTION_GRID`, appended after the severities entry for an endpoint that is off the grid.
  if (c.connectionGrid) out.push({ id: "connection_grid", label: "Edit connection grid spacing...", hint: `Open the ${setup} dialog` });
  if (c.onCanvas) {
    out.push({ id: "separator", label: "" });
    out.push({ id: "show_in_dialog", label: c.domain === "drc" ? "Show in the Design Rules Checker" : "Show in the Electrical Rules Checker" });
  }
  return out;
}

// ------------------------------------------------------------------------------------------------------------ Inspect and Fix

/** What an Inspect or a Fix entry does when it is picked: run an action (after selecting the violation's main item when the action works on the selection), or open the item's own properties. */
export type MarkerCommand =
  | { kind: "action"; id: string; select: boolean }
  | { kind: "properties" }
  | { kind: "inspect_clearance" }
  | { kind: "inspect_constraints" };

/** One of those entries: the text of the menu item (`GetRunMenuCommandDescription` of the action, or the dialog's own words) and what it does. */
export interface MarkerEntry {
  label: string;
  command: MarkerCommand;
}

/** `BOARD_INSPECTION_TOOL::InspectDRCErrorMenuText`: the checks whose violation a Clearance Resolution report explains. */
const CLEARANCE_CHECKS: readonly string[] = ["clearance", "copper_edge_clearance", "hole_clearance", "hole_to_hole", "starved_thermal"];
/** ... and a Constraints Resolution report. */
const CONSTRAINT_CHECKS: readonly string[] = [
  "text_height",
  "text_thickness",
  "diff_pair_uncoupled_length_too_long",
  "track_width",
  "track_angle",
  "track_segment_length",
  "via_diameter",
  "annular_width",
  "drill_out_of_range",
  "microvia_drill_out_of_range",
  "connection_width",
  "assertion_failure",
];

/** `InspectDRCErrorMenuText` and `InspectDRCError`: the DRC marker's Inspect entry, if its check has one. */
export function drcInspectEntry(check: string): MarkerEntry | null {
  if (CLEARANCE_CHECKS.includes(check)) return { label: "Clearance Resolution", command: { kind: "inspect_clearance" } };
  if (CONSTRAINT_CHECKS.includes(check)) return { label: "Constraints Resolution", command: { kind: "inspect_constraints" } };
  if (check === "lib_footprint_mismatch") return { label: "Compare Footprint with Library", command: { kind: "action", id: "pcbnew.InspectionTool.DiffFootprint", select: true } };
  return null;
}

/**
 * `DRC_TOOL::FixDRCErrorMenuText` and `FixDRCError`: the DRC marker's Fix entry, if its check has one. `mainKind` is what the violation's main item is
 * (`itemKind`: a text, a dimension ...); a text check's entry says Dimension, Field or Text Properties after it.
 */
export function drcFixEntry(check: string, mainKind: string | null): MarkerEntry | null {
  const act = (label: string, id: string, select = false): MarkerEntry => ({ label, command: { kind: "action", id, select } });
  const props = (label: string): MarkerEntry => ({ label, command: { kind: "properties" } });
  switch (check) {
    case "lib_footprint_issues":
      return act("Manage Footprint Libraries...", "common.SuiteControl.showFootprintLibTable");
    case "lib_footprint_mismatch":
      return act("Update Footprint...", "pcbnew.GlobalEdit.updateFootprint", true);
    case "footprint_filters_mismatch":
      return act("Change Footprint...", "pcbnew.GlobalEdit.changeFootprint", true);
    case "footprint_symbol_mismatch":
    case "footprint_symbol_field_mismatch":
    case "missing_footprint":
    case "duplicate_footprints":
    case "extra_footprint":
      return act("Update PCB from Schematic...", "common.Control.updatePcbFromSchematic");
    case "footprint_type_mismatch":
    case "footprint":
      return props("Edit Footprint Properties...");
    case "padstack":
    case "padstack_invalid":
      return props("Edit Pad Properties...");
    case "text_height":
    case "text_thickness":
    case "mirrored_text_on_front_layer":
    case "nonmirrored_text_on_back_layer":
      return props(mainKind === "dimension" ? "Edit Dimension Properties..." : mainKind === "field" ? "Edit Field Properties..." : "Edit Text Properties...");
    case "track_dangling":
    case "via_dangling":
      return act("Cleanup Tracks & Vias...", "pcbnew.GlobalEdit.cleanupTracksAndVias");
    default:
      return null;
  }
}

/** `SCH_INSPECTION_TOOL::InspectERCErrorMenuText` and `InspectERCError`. */
export function ercInspectEntry(check: string): MarkerEntry | null {
  if (check === "bus_to_net_conflict") return { label: "Show Bus Syntax Help", command: { kind: "action", id: "eeschema.InspectionTool.showBusSyntaxHelp", select: false } };
  if (check === "lib_symbol_mismatch") return { label: "Compare Symbol with Library", command: { kind: "action", id: "eeschema.InspectionTool.diffSymbol", select: true } };
  return null;
}

/** `SCH_EDIT_TOOL::FixERCErrorMenuText` and `FixERCError` (without "Edit Netclasses...", which needs Schematic Setup's Net Classes page). */
export function ercFixEntry(check: string): MarkerEntry | null {
  const act = (label: string, id: string, select = false): MarkerEntry => ({ label, command: { kind: "action", id, select } });
  switch (check) {
    case "simulation_model_issue":
    case "footprint_filter":
    case "footprint_link_issues":
      return act("Edit Symbol Properties...", "eeschema.InteractiveEdit.properties", true);
    case "lib_symbol_issues":
      return act("Manage Symbol Libraries...", "common.SuiteControl.showSymbolLibTable");
    case "lib_symbol_mismatch":
      return act("Update Symbol...", "eeschema.InteractiveEdit.updateSymbol", true);
    case "unannotated":
    case "duplicate_reference":
      return act("Annotate Schematic...", "eeschema.EditorControl.annotate");
    default:
      return null;
  }
}

// ------------------------------------------------------------------------------------------------------------ the rule a violation broke

/**
 * The custom rule a DRC violation broke: `DRC_ITEM::GetViolatingRule()->m_Name` when the rule is not implicit (`dialog_drc.cpp` offers "Exclude all
 * violations of rule" for those only). kicad-cli's report does not carry the rule but its text names it: a constraint found in a custom rule reports
 * as "rule '<name>'" (`DRC_CONSTRAINT::GetName`); an implicit rule's name stands bare. Null when the violation names none.
 */
export function violatingRule(description: string): string | null {
  const m = /\brule '([^']*)'/.exec(description);
  return m ? (m[1] ?? null) : null;
}

/** The violations of one rule on a page of the report, as "Exclude all violations of rule" works on them: the ones not waived yet (to exclude) or the ones that are (to remove). */
export function ofRule(list: readonly DrcViolation[], rule: string, excluded: boolean): DrcViolation[] {
  return list.filter((v) => violatingRule(v.description) === rule && canExclude(v) && (v.excluded ?? false) === excluded);
}

/** `ID_SET_SEVERITY_TO_*` -> the severity it sets. */
export function menuSeverity(id: RcMenuId): RuleSeverity | null {
  return id === "severity_error" ? "error" : id === "severity_warning" ? "warning" : id === "severity_ignore" ? "ignore" : null;
}

/**
 * The table `set_rule_severities` / `set_erc_severities` is sent after one check's severity changes (a marker menu's "Change severity ...",
 * the Ignored Tests tab's radio menu): every check keeps the severity it has, `key` takes `severity`, and only what differs from KiCad's
 * default -- or is named already -- is sent (`severitiesToSend`).
 */
export function severitiesAfter(items: readonly SeverityItem[], current: Readonly<Record<string, RuleSeverity>>, key: string, severity: RuleSeverity): Record<string, RuleSeverity> {
  const chosen: Record<string, RuleSeverity> = {};
  for (const item of items) chosen[item.key] = severityOf(item, current);
  chosen[key] = severity;
  return severitiesToSend(items, chosen, current);
}

// ------------------------------------------------------------------------------------------------------------ DRC exclusions

/** `DrcExclusion::key` of a violation: the check and the uuids of the items, main first -- what the server matches a waived violation by. */
export function drcKey(v: Pick<DrcViolation, "type" | "items">): { check: string; items: string[] } {
  return { check: v.type, items: v.items.map((i) => i.uuid ?? "") };
}

/** The `add_drc_exclusions` entry for a violation: what the report knows (the items' uuids and ids, the marker positions worth trying) and a comment. */
export function drcExclusionSpec(v: DrcViolation, comment = ""): DrcExclusionSpec {
  const key = drcKey(v);
  const spec: DrcExclusionSpec = { check: key.check, items: key.items, ids: v.items.map((i) => i.id ?? "") };
  if (v.marker_nm && v.marker_nm.length > 0) spec.positions_nm = v.marker_nm;
  if (comment !== "") spec.comment = comment;
  return spec;
}

/** Whether a violation can be waived: it names the items to key the waiver on (a lint finding does not; neither does a report from an older server). */
export function canExclude(v: Pick<DrcViolation, "items">): boolean {
  return v.items.length > 0 && v.items.every((i) => (i.uuid ?? "") !== "");
}

/** The violations of one check, as `ExcludeAll` / `RemoveAll` work on them: the ones not waived yet (to exclude) or the ones that are (to remove). */
export function ofCheck(list: readonly DrcViolation[], check: string, excluded: boolean): DrcViolation[] {
  return list.filter((v) => v.type === check && canExclude(v) && (v.excluded ?? false) === excluded);
}

const sameKey = (a: { check: string; items: string[] }, b: { check: string; items: string[] }) => a.check === b.check && a.items.length === b.items.length && a.items.every((x, i) => x === b.items[i]);

/**
 * The report after waiving (or restoring) violations, without running kicad-cli again: `ERC_MARK_EXCLUDED`'s DRC twin. `comment` is the
 * exclusion's. The counts leave the waived ones out. Whether kicad-cli matched the new exclusion is not known until the next run.
 */
export function patchDrcExcluded(report: DrcReport, keys: ReadonlyArray<{ check: string; items: string[] }>, excluded: boolean, comment = "", hold = false): DrcReport {
  // `hold`: one marker waived or restored from its row (`ValueChanged( node )`), which keeps its place in the list; otherwise the dialog's `Update`
  // refilters and every row is sorted afresh (r2 D-E3, [`listedIndexes`])
  const patch = (v: DrcViolation): DrcViolation => {
    const key = drcKey(v);
    const { listed_as: was, ...unheld } = v;
    if (!keys.some((k) => sameKey(k, key))) return hold ? v : unheld;
    const { kicad_matched: _unknown, ...rest } = unheld;
    return { ...rest, excluded, comment: excluded ? comment : "", ...(hold ? { listed_as: was ?? rcKind(v) } : {}) };
  };
  const violations = report.violations.map(patch);
  const unconnected = report.unconnected_items?.map(patch);
  const parity = report.schematic_parity?.map(patch);
  const counts: Record<string, number> = {};
  for (const v of [...violations, ...(unconnected ?? []), ...(parity ?? [])]) if (!v.excluded) counts[v.type] = (counts[v.type] ?? 0) + 1;
  return { ...report, violations, ...(unconnected ? { unconnected_items: unconnected } : {}), ...(parity ? { schematic_parity: parity } : {}), counts };
}

/** `RC_TREE_MODEL::Update` on the DRC dialog (a change of the Show boxes, a severity, a bulk exclusion): the provider refilters and sorts every row by
 *  what it is now, so no row keeps a place it was given before (r2 D-E3). */
export function refilterDrc(report: DrcReport): DrcReport {
  const clear = (list: DrcViolation[] | undefined) => list?.map(({ listed_as: _held, ...v }) => v);
  return {
    ...report,
    violations: clear(report.violations) ?? [],
    ...(report.unconnected_items ? { unconnected_items: clear(report.unconnected_items) } : {}),
    ...(report.schematic_parity ? { schematic_parity: clear(report.schematic_parity) } : {}),
  };
}

/**
 * The report after a check's severity changed (`ID_SET_SEVERITY_TO_*`, the Ignored Tests radio menu), without running kicad-cli again --
 * what KiCad does to its markers in place: a check set to Error or Warning keeps its markers at the new severity; one set to Ignore loses
 * them and joins the ignored tests (`m_ignoredList`), and a check taken off Ignore leaves that list. The counts follow.
 */
export function patchDrcSeverity(report: DrcReport, check: string, severity: RuleSeverity, description: string): DrcReport {
  const ignoring = severity === "ignore";
  // `ID_SET_SEVERITY_TO_*` end with `Update`, a refilter: no row keeps a held place (r2 D-E3)
  const patch = (list: DrcViolation[] | undefined) => list?.filter((v) => !(ignoring && v.type === check)).map(({ listed_as: _held, ...v }) => (v.type === check && !ignoring ? { ...v, severity } : v));
  const violations = patch(report.violations) ?? [];
  const unconnected = patch(report.unconnected_items);
  const parity = patch(report.schematic_parity);
  const counts: Record<string, number> = {};
  for (const v of [...violations, ...(unconnected ?? []), ...(parity ?? [])]) if (!v.excluded) counts[v.type] = (counts[v.type] ?? 0) + 1;
  const rest = (report.ignored_checks ?? []).filter((c) => c.key !== check);
  return {
    ...report,
    violations,
    ...(unconnected ? { unconnected_items: unconnected } : {}),
    ...(parity ? { schematic_parity: parity } : {}),
    counts,
    ignored_checks: ignoring ? [...rest, { key: check, description }] : rest,
  };
}

// ------------------------------------------------------------------------------------------------------------ ERC exclusions

/** An ERC exclusion's key: `(check, location)`, and the sheet path of the finding's marker when it has one (`ErcMarker.sheet`). */
export type ErcKey = { check: string; location: string; sheet?: string[] };

/**
 * The key `add_erc_exclusion` takes for a finding; null for one with nothing to key on. A screen two sheets show has one id per item, so its placements'
 * findings share `(check, location)`: the marker's sheet (`SetSheetSpecificPath`) tells them apart.
 */
export function ercKey(v: Pick<ErcViolation, "check" | "location"> & { marker?: ErcViolation["marker"] }): ErcKey | null {
  if (!v.location) return null;
  const sheet = v.marker?.sheet;
  return sheet && sheet.length > 0 ? { check: v.check, location: v.location, sheet } : { check: v.check, location: v.location };
}

/** Whether a finding is the one `k` keys: its check and location, and its marker's sheet when the key has one. */
function ercKeyHits(k: ErcKey, v: ErcViolation): boolean {
  if (v.location === null || k.check !== v.check || k.location !== v.location) return false;
  if (!k.sheet) return true;
  const sheet = v.marker?.sheet ?? [];
  return sheet.length === 0 || (sheet.length === k.sheet.length && sheet.every((u, i) => u === k.sheet![i]));
}

/**
 * The report after waiving (or restoring) findings, without running kicad-cli again: [`patchDrcExcluded`]'s ERC twin. `comment` is the exclusion's
 * (`SCH_MARKER::GetComment`). The counts leave the waived ones out. Whether kicad-cli matched the new exclusion is not known until the next run.
 */
export function patchErcExcluded(report: ErcReport, keys: ReadonlyArray<ErcKey>, excluded: boolean, comment = ""): ErcReport {
  const hit = (v: ErcViolation) => keys.some((k) => ercKeyHits(k, v));
  const violations = report.violations.map((v) => {
    if (!hit(v)) return v;
    const { kicad_matched: _unknown, ...rest } = v;
    return { ...rest, severity: excluded ? ("excluded" as const) : (v.base_severity ?? ("error" as const)), comment: excluded ? comment : "" };
  });
  const counts: Record<string, number> = {};
  for (const v of violations) if (v.severity !== "excluded") counts[v.check] = (counts[v.check] ?? 0) + 1;
  return { ...report, violations, counts };
}

// ------------------------------------------------------------------------------------------------------------ Delete Marker, Delete All Markers

/** The pages of the DRC dialog that list markers. */
export type DrcPage = "violations" | "unconnected" | "parity";

const dropAt = <T,>(list: readonly T[] | undefined, index: number): T[] | undefined => list?.filter((_, i) => i !== index);

function drcCounts(...lists: ReadonlyArray<readonly DrcViolation[] | undefined>): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const list of lists) for (const v of list ?? []) if (!v.excluded) counts[v.type] = (counts[v.type] ?? 0) + 1;
  return counts;
}

/**
 * Delete Marker on a page of the DRC report (`DIALOG_DRC::OnDeleteOneClick`, `RC_TREE_MODEL::DeleteCurrentItem( true )`): the marker leaves the list and the
 * board, whatever it is, an exclusion included. A violation is not fixed by it: the next run finds it. Whether an excluded marker's exclusion goes with it is the
 * caller's (KiCad records the markers that are left, `BOARD::RecordDRCExclusions`, so it does: `deleteMarkerDrc` sends `delete_drc_exclusions`).
 */
export function patchDrcDeleteMarker(report: DrcReport, page: DrcPage, index: number): DrcReport {
  const violations = page === "violations" ? (dropAt(report.violations, index) ?? []) : report.violations;
  const unconnected = page === "unconnected" ? dropAt(report.unconnected_items, index) : report.unconnected_items;
  const parity = page === "parity" ? dropAt(report.schematic_parity, index) : report.schematic_parity;
  return { ...report, violations, ...(unconnected ? { unconnected_items: unconnected } : {}), ...(parity ? { schematic_parity: parity } : {}), counts: drcCounts(violations, unconnected, parity) };
}

/**
 * Delete All Markers (`DIALOG_DRC::OnDeleteAllClick`, `deleteAllMarkers`): the errors and warnings of every page go; the exclusions go too when the person said so
 * ("Errors, Warnings and Exclusions"). The report is left as a run that has to be made again (`m_drcRun = false`).
 */
export function patchDrcDeleteAll(report: DrcReport, includeExclusions: boolean): DrcReport {
  const keep = (list: DrcViolation[] | undefined) => list?.filter((v) => !includeExclusions && (v.excluded ?? false));
  const violations = keep(report.violations) ?? [];
  const unconnected = keep(report.unconnected_items);
  const parity = keep(report.schematic_parity);
  return { ...report, violations, ...(unconnected ? { unconnected_items: unconnected } : {}), ...(parity ? { schematic_parity: parity } : {}), counts: {}, markers_deleted: true };
}

function ercCounts(violations: readonly ErcViolation[]): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const v of violations) if (v.severity !== "excluded") counts[v.check] = (counts[v.check] ?? 0) + 1;
  return counts;
}

/**
 * Delete Marker on the ERC report (`DIALOG_ERC::OnDeleteOneClick`). The report on screen loses the row; whether an excluded marker's exclusion goes with it is
 * the caller's (KiCad records the markers that are left, `RecordERCExclusions`, so it does: `deleteErcMarker` sends `delete_erc_exclusion`).
 */
export function patchErcDeleteMarker(report: ErcReport, index: number): ErcReport {
  const violations = report.violations.filter((_, i) => i !== index);
  return { ...report, violations, counts: ercCounts(violations) };
}

/** Delete All Markers on the ERC report (`DIALOG_ERC::OnDeleteAllClick`): [`patchDrcDeleteAll`]'s twin. */
export function patchErcDeleteAll(report: ErcReport, includeExclusions: boolean): ErcReport {
  const violations = report.violations.filter((v) => !includeExclusions && v.severity === "excluded");
  return { ...report, violations, counts: {}, markers_deleted: true };
}

/** [`patchDrcSeverity`] for the schematic: an excluded finding stays excluded, whatever the check becomes. */
export function patchErcSeverity(report: ErcReport, check: string, severity: RuleSeverity, description: string): ErcReport {
  const ignoring = severity === "ignore";
  const violations = report.violations.filter((v) => !(ignoring && v.check === check)).map((v) => (v.check === check && !ignoring && v.severity !== "excluded" ? { ...v, severity } : v));
  const counts: Record<string, number> = {};
  for (const v of violations) if (v.severity !== "excluded") counts[v.check] = (counts[v.check] ?? 0) + 1;
  const rest = (report.ignored_checks ?? []).filter((c) => c.key !== check);
  return { ...report, violations, counts, ignored_checks: ignoring ? [...rest, { key: check, description }] : rest };
}
