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
// What differs from KiCad, and why:
//   - "Exclude all violations of rule '...'" is per violated *rule* in KiCad (a custom rule's name; DRC only); kicad-cli's report does not say which
//     rule a violation broke, so the menu offers "all '<check>' violations" -- every violation of the check -- on both the board and the schematic.
//   - The ERC menu has no "Fix"/"Inspect" entries: those open KiCad's own library table and symbol diff dialogs.
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

/** The indexes of the entries the list shows, in report order. */
export function listedIndexes<T extends { severity: string; excluded?: boolean }>(entries: readonly T[], f: RcFilter): number[] {
  const out: number[] = [];
  entries.forEach((e, i) => {
    if (isListed(rcKind(e), f)) out.push(i);
  });
  return out;
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
  | "exclude"
  | "exclude_comment"
  | "exclude_type"
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
  /** An exclusion can carry a comment (`m_DrcExclusionComments`). The board's can; the schematic's here is a bare `(check, location)` key, so its menu has no comment entries. Default true. */
  comments?: boolean;
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
    out.push({ id: "remove_exclusion_type", label: `Remove all exclusions for '${c.title}' violations`, hint: `They will be placed back in the ${listName} list` });
  } else {
    out.push({ id: "exclude", label: "Exclude this violation", hint: `It will be excluded from the ${listName} list` });
    if (comments) out.push({ id: "exclude_comment", label: "Exclude with comment...", hint: `It will be excluded from the ${listName} list` });
    out.push({ id: "exclude_type", label: `Exclude all '${c.title}' violations`, hint: `They will be excluded from the ${listName} list` });
  }
  out.push({ id: "separator", label: "" });
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
  if (c.onCanvas) {
    out.push({ id: "separator", label: "" });
    out.push({ id: "show_in_dialog", label: c.domain === "drc" ? "Show in the Design Rules Checker" : "Show in the Electrical Rules Checker" });
  }
  return out;
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
export function patchDrcExcluded(report: DrcReport, keys: ReadonlyArray<{ check: string; items: string[] }>, excluded: boolean, comment = ""): DrcReport {
  const patch = (v: DrcViolation): DrcViolation => {
    const key = drcKey(v);
    if (!keys.some((k) => sameKey(k, key))) return v;
    const { kicad_matched: _unknown, ...rest } = v;
    return { ...rest, excluded, comment: excluded ? comment : "" };
  };
  const violations = report.violations.map(patch);
  const unconnected = report.unconnected_items?.map(patch);
  const parity = report.schematic_parity?.map(patch);
  const counts: Record<string, number> = {};
  for (const v of [...violations, ...(unconnected ?? []), ...(parity ?? [])]) if (!v.excluded) counts[v.type] = (counts[v.type] ?? 0) + 1;
  return { ...report, violations, ...(unconnected ? { unconnected_items: unconnected } : {}), ...(parity ? { schematic_parity: parity } : {}), counts };
}

/**
 * The report after a check's severity changed (`ID_SET_SEVERITY_TO_*`, the Ignored Tests radio menu), without running kicad-cli again --
 * what KiCad does to its markers in place: a check set to Error or Warning keeps its markers at the new severity; one set to Ignore loses
 * them and joins the ignored tests (`m_ignoredList`), and a check taken off Ignore leaves that list. The counts follow.
 */
export function patchDrcSeverity(report: DrcReport, check: string, severity: RuleSeverity, description: string): DrcReport {
  const ignoring = severity === "ignore";
  const patch = (list: DrcViolation[] | undefined) => list?.filter((v) => !(ignoring && v.type === check)).map((v) => (v.type === check && !ignoring ? { ...v, severity } : v));
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

/** The `(check, location)` key `add_erc_exclusion` takes for a finding; null for one with nothing to key on. */
export function ercKey(v: Pick<ErcViolation, "check" | "location">): { check: string; location: string } | null {
  return v.location ? { check: v.check, location: v.location } : null;
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
