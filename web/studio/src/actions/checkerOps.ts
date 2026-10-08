// What the Checker windows and the canvas markers do to a marker: select it (cross-probe), step to the next one, waive it, change its check's
// severity, bring it up in the dialog. One place for the DRC dialog, the ERC dialog, the Inspect menu's Next / Previous / Exclude Marker actions
// (actions/commonCheckerActions.ts) and the marker menus on the two canvases, so they cannot disagree.
//
//   pcbnew/dialogs/dialog_drc.cpp      OnDRCItemSelected (cross-probe), OnDRCItemRClick (the menu's commands), NextMarker, ExcludeMarker, SelectMarker
//   eeschema/dialogs/dialog_erc.cpp    OnERCItemSelected, OnERCItemRClick, NextMarker, ExcludeMarker, SelectMarker
//   pcbnew/tools/drc_tool.cpp          DRC_TOOL::NextMarker / PrevMarker / ExcludeMarker / CrossProbe
//   eeschema/tools/sch_inspection_tool.cpp  SCH_INSPECTION_TOOL::NextMarker / PrevMarker / ExcludeMarker / CrossProbe
//
// The pure rules are kicad-port/rcItems.ts; this file is the part that talks to the store and the server.
import type { Dispatch } from "react";
import { fetchErcSeverities, fetchVersion } from "../api/client";
import type { Cmd, DrcReport, DrcViolation, ErcViolation, RuleSeverity } from "../api/types";
import drcChecks from "../kicad/drc_checks.json";
import ercChecks from "../kicad/erc_checks.json";
import { revisionAfterOwnEdit } from "../kicad-port/checkRevision";
import { canExclude, drcExclusionSpec, drcKey, ercKey, listedIndexes, menuSeverity, ofCheck, selectionAfterRowGone, severitiesAfter, stepListed, type RcMenuEntry, type RcMenuId } from "../kicad-port/rcItems";
import { fitTransform } from "../kicad-port/view";
import { ercMarkerPosition } from "../components/schematic/ercMarkerPosition";
import { getCheckerView, setDrcView, setErcView, type DrcTab } from "../state/checkerView";
import type { Action as StudioAction, StudioApi, StudioState } from "../state/store";
import type { MenuEntry } from "../components/canvas/ContextMenu";
import { canvasRect } from "./canvasEvents";

export interface CheckerCtx {
  api: StudioApi;
  dispatch: Dispatch<StudioAction>;
}

// -------------------------------------------------------------------------------------------------------------------------- the checks

interface CheckRow {
  key: string;
  title: string;
  defaultSeverity: RuleSeverity;
}

/** `DRC_ITEM::GetItemsWithSeverities`, flat (src/kicad/drc_checks.json). */
export const DRC_CHECK_ITEMS: readonly CheckRow[] = (drcChecks.groups as ReadonlyArray<{ items: CheckRow[] }>).flatMap((g) => g.items);
/** `ERC_ITEM::GetItemsWithSeverities`, flat, the pin conflicts map's row last (src/kicad/erc_checks.json). */
export const ERC_CHECK_ITEMS: readonly CheckRow[] = [...(ercChecks.groups as ReadonlyArray<{ items: CheckRow[] }>).flatMap((g) => g.items), ercChecks.pinMap as CheckRow];

const titleIn = (rows: readonly CheckRow[], key: string) => rows.find((r) => r.key === key)?.title ?? key.replace(/_/g, " ");
/** `RC_ITEM::GetErrorText( true )` of a DRC check: its name in Board Setup > Violation Severity. */
export const drcTitle = (key: string): string => titleIn(DRC_CHECK_ITEMS, key);
/** The same for an ERC check. */
export const ercTitle = (key: string): string => titleIn(ERC_CHECK_ITEMS, key);

/** The severity setting of a DRC check right now: the board's table when it names the check, else KiCad's default. */
export function drcSettingSeverity(state: Pick<StudioState, "board">, key: string): RuleSeverity {
  const table = state.board?.board_rules?.severities ?? {};
  return table[key] ?? DRC_CHECK_ITEMS.find((r) => r.key === key)?.defaultSeverity ?? "error";
}

/** The severity setting of an ERC check: what the finding is when not excluded, else the table last read, else KiCad's default. */
export function ercSettingSeverity(v: Pick<ErcViolation, "check" | "severity" | "base_severity">): RuleSeverity {
  if (v.severity === "error" || v.severity === "warning") return v.severity;
  const table = getCheckerView().erc.severities ?? {};
  return v.base_severity ?? table[v.check] ?? ERC_CHECK_ITEMS.find((r) => r.key === v.check)?.defaultSeverity ?? "error";
}

/** `ERCE_PIN_TO_PIN_WARNING` / `_ERROR`: findings of the pin conflicts map, whose severity is the map's. */
export const isPinMapCheck = (check: string): boolean => check === "pin_to_pin";

// -------------------------------------------------------------------------------------------------------------------------- selecting

/** Our id for a violation's item -> the id the canvas selects by: a pad (`REF.PAD`) selects its part, a track segment (`id#n`) its track; the Edge.Cuts outline is nothing to select. */
const baseRef = (id: string) => id.split("#")[0]!.split(".")[0]!;

/** A close-up needs some room around a single point: 2 mm either side. */
const FRAME_PAD_UM = 2_000;

/** The list of the DRC report a notebook page shows (the Lint and Ignored Tests pages are not lists of markers). */
export function drcListOf(report: DrcReport | null, tab: DrcTab): DrcViolation[] {
  if (!report) return [];
  if (tab === "violations") return report.violations;
  if (tab === "unconnected") return report.unconnected_items ?? [];
  if (tab === "parity") return report.schematic_parity ?? [];
  return [];
}

/** The row selected on a page of the DRC dialog. */
export function drcSelectedOf(state: Pick<StudioState, "drcSelected">, tab: DrcTab): number | null {
  const v = getCheckerView().drc;
  return tab === "violations" ? state.drcSelected : tab === "unconnected" ? v.unconnectedSelected : tab === "parity" ? v.paritySelected : null;
}

/**
 * `DIALOG_DRC::OnDRCItemSelected`: a marker clicked in the list selects the items it names on the board and frames them (the same click the real
 * dialog turns into `FocusOnItems`). `showPcb`: also bring the PCB tab up, for a click that may come from another editor.
 */
export function selectDrc(ctx: CheckerCtx, tab: DrcTab, index: number, showPcb = false): void {
  const { api, dispatch } = ctx;
  const v = drcListOf(api.getState().drc, tab)[index];
  if (tab === "violations") dispatch({ type: "SET_DRC_SELECTED", index });
  else {
    dispatch({ type: "SET_DRC_SELECTED", index: null });
    setDrcView(tab === "unconnected" ? { unconnectedSelected: index } : { paritySelected: index });
  }
  if (!v) return;
  const refs = v.items.flatMap((it) => (it.id && it.id !== "outline" ? [baseRef(it.id)] : []));
  dispatch({ type: "SET_SELECTION", refs });
  dispatch({ type: "SET_HOT", refs });
  const rect = canvasRect();
  if (rect && rect.width >= 50 && rect.height >= 50 && v.items.length > 0) {
    const xs = v.items.map((it) => it.pos[0]);
    const ys = v.items.map((it) => it.pos[1]);
    const bounds = { minX: Math.min(...xs) - FRAME_PAD_UM, minY: Math.min(...ys) - FRAME_PAD_UM, maxX: Math.max(...xs) + FRAME_PAD_UM, maxY: Math.max(...ys) + FRAME_PAD_UM };
    dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height, 60) });
  }
  if (showPcb) dispatch({ type: "SET_TAB", tab: "pcb" });
}

/**
 * What a click on an ERC row does to the canvas, whichever list the row is in: select the symbol the finding names and frame the point its `location`
 * resolves to (`ercMarkerPosition`). A location it cannot resolve (an item the schematic no longer has) leaves the view where it is.
 */
export function frameErc(ctx: CheckerCtx, v: Pick<ErcViolation, "location"> | undefined, showSchematic = false): void {
  const { api, dispatch } = ctx;
  const resolved = v ? ercMarkerPosition(v.location, api.getState().schematic) : null;
  if (resolved) {
    if (resolved.refs.length > 0) {
      dispatch({ type: "SET_SELECTION", refs: resolved.refs });
      dispatch({ type: "SET_HOT", refs: resolved.refs });
    }
    const rect = canvasRect();
    if (rect && rect.width >= 50 && rect.height >= 50) {
      const [x, y] = resolved.at;
      dispatch({ type: "SET_SCHEMATIC_VIEW", view: fitTransform({ minX: x - FRAME_PAD_UM, minY: y - FRAME_PAD_UM, maxX: x + FRAME_PAD_UM, maxY: y + FRAME_PAD_UM }, rect.width, rect.height, 60) });
    }
  }
  if (showSchematic) dispatch({ type: "SET_TAB", tab: "schematic" });
}

/** `DIALOG_ERC::OnERCItemSelected`: select the row, and its finding on the canvas ([`frameErc`]). */
export function selectErc(ctx: CheckerCtx, index: number, showSchematic = false): void {
  ctx.dispatch({ type: "SET_ERC_SELECTED", index });
  frameErc(ctx, ctx.api.getState().erc?.violations[index], showSchematic);
}

/**
 * `DRC_TOOL::NextMarker` / `PrevMarker`: with the dialog up, the next (previous) marker of the page it shows, among the rows its Show boxes list;
 * without it, the dialog comes up and that is all (`ShowDRCDialog`). The Ignored Tests page has no markers (`case 3: break`).
 */
export function stepDrc(ctx: CheckerCtx, dir: "next" | "prev"): void {
  const { api, dispatch } = ctx;
  const state = api.getState();
  if (!state.drcDialogOpen) {
    dispatch({ type: "SET_DRC_OPEN", open: true });
    return;
  }
  const { tab, filter } = getCheckerView().drc;
  if (tab === "ignored" || tab === "lint") return;
  const list = drcListOf(state.drc, tab);
  const to = stepListed(listedIndexes(list, filter), drcSelectedOf(state, tab), dir);
  if (to !== null) selectDrc(ctx, tab, to);
}

/** `SCH_INSPECTION_TOOL::NextMarker` / `PrevMarker`: the dialog comes up on its Violations page and steps its list. */
export function stepErc(ctx: CheckerCtx, dir: "next" | "prev"): void {
  const { api, dispatch } = ctx;
  const state = api.getState();
  if (!state.ercDialogOpen) dispatch({ type: "SET_ERC_DIALOG_OPEN", open: true });
  setErcView({ tab: "erc" });
  const list = state.erc?.violations ?? [];
  const to = stepListed(listedIndexes(list, getCheckerView().erc.filter), state.ercSelected, dir);
  if (to !== null) selectErc(ctx, to);
}

/**
 * `DRC_TOOL::CrossProbe( marker )` / `SCH_INSPECTION_TOOL::CrossProbe`: the canvas marker's "Show in the dialog" -- the dialog comes up with the
 * marker's row selected (on the page that lists it).
 */
export function showDrcInDialog(ctx: CheckerCtx, tab: DrcTab, index: number): void {
  ctx.dispatch({ type: "SET_DRC_OPEN", open: true });
  setDrcView({ tab });
  selectDrc(ctx, tab, index);
}

export function showErcInDialog(ctx: CheckerCtx, index: number): void {
  ctx.dispatch({ type: "SET_ERC_DIALOG_OPEN", open: true });
  setErcView({ tab: "erc" });
  selectErc(ctx, index);
}

// -------------------------------------------------------------------------------------------------------------------------- waiving

/** The revision a patched report is current for: the design's after the edit when the report was current before it (checkRevision.ts's `revisionAfterOwnEdit`). */
async function revisionAfter(reportVersion: string | null, before: StudioState): Promise<string | null> {
  const boardAfter = await fetchVersion().catch(() => null);
  return revisionAfterOwnEdit(reportVersion, before.version, boardAfter);
}

/**
 * "Exclude this violation" / "Exclude with comment..." / "Exclude all ..." (and Exclude Marker) on the board: one undo step. The report on screen
 * is patched in place -- a waiver does not make a current report out of date.
 */
export async function excludeDrc(ctx: CheckerCtx, violations: readonly DrcViolation[], comment = ""): Promise<boolean> {
  const { api, dispatch } = ctx;
  const todo = violations.filter(canExclude);
  if (todo.length === 0) return false;
  const before = api.getState();
  if (!(await api.cmd({ op: "add_drc_exclusions", exclusions: todo.map((v) => drcExclusionSpec(v, comment)) }))) return false;
  const version = await revisionAfter(before.drcVersion, before);
  dispatch({ type: "DRC_PATCH_EXCLUDED", keys: todo.map(drcKey), excluded: true, comment, version });
  return true;
}

/** "Remove exclusion for this violation" (and "Remove all exclusions ..."). */
export async function restoreDrc(ctx: CheckerCtx, violations: readonly DrcViolation[]): Promise<boolean> {
  const { api, dispatch } = ctx;
  const todo = violations.filter(canExclude);
  if (todo.length === 0) return false;
  const before = api.getState();
  if (!(await api.cmd({ op: "delete_drc_exclusions", exclusions: todo.map(drcKey) }))) return false;
  const version = await revisionAfter(before.drcVersion, before);
  dispatch({ type: "DRC_PATCH_EXCLUDED", keys: todo.map(drcKey), excluded: false, comment: "", version });
  return true;
}

/** Waive (or restore) every violation of a check on every page of the report: "Exclude all '...' violations". */
export async function excludeDrcOfCheck(ctx: CheckerCtx, check: string, excluded: boolean): Promise<boolean> {
  const report = ctx.api.getState().drc;
  if (!report) return false;
  const all = [...report.violations, ...(report.unconnected_items ?? []), ...(report.schematic_parity ?? [])];
  const of = ofCheck(all, check, !excluded);
  return excluded ? excludeDrc(ctx, of) : restoreDrc(ctx, of);
}

/** `ERC` twins: the exclusion is a bare `(check, location)` key, applied to the report on screen. */
export async function setErcExcluded(ctx: CheckerCtx, violations: readonly ErcViolation[], excluded: boolean): Promise<boolean> {
  const { api, dispatch } = ctx;
  const keys = violations.flatMap((v) => (excluded ? v.severity !== "excluded" : v.severity === "excluded") ? [ercKey(v)] : []).filter((k): k is { check: string; location: string } => k !== null);
  if (keys.length === 0) return false;
  const before = api.getState();
  const cmds: Cmd[] = keys.map((k) => ({ op: excluded ? "add_erc_exclusion" : "delete_erc_exclusion", check: k.check, location: k.location }));
  if (!(await api.cmdBatch(cmds))) return false;
  const version = await revisionAfter(before.ercVersion, before);
  for (const k of keys) dispatch({ type: "ERC_MARK_EXCLUDED", check: k.check, location: k.location, excluded, version });
  return true;
}

export async function excludeErcOfCheck(ctx: CheckerCtx, check: string, excluded: boolean): Promise<boolean> {
  const list = ctx.api.getState().erc?.violations ?? [];
  return setErcExcluded(ctx, list.filter((v) => v.check === check), excluded);
}

/**
 * `DIALOG_DRC::ExcludeMarker` (Exclude Marker, `ACTIONS::excludeMarker`): the Violations page's selected marker, when it is not waived yet. With the Show boxes
 * leaving exclusions out the row leaves the list and the selection moves on to the next one (`RC_TREE_MODEL::DeleteCurrentItem`).
 */
export async function excludeMarkerDrc(ctx: CheckerCtx): Promise<void> {
  const s = ctx.api.getState();
  if (!s.drcDialogOpen || getCheckerView().drc.tab !== "violations") return;
  const list = s.drc?.violations ?? [];
  const here = s.drcSelected;
  const v = here !== null ? list[here] : undefined;
  if (here === null || !v || v.excluded) return;
  const filter = getCheckerView().drc.filter;
  const listed = listedIndexes(list, filter);
  if (!(await excludeDrc(ctx, [v]))) return;
  const next = filter.exclusions ? null : selectionAfterRowGone(listed, here);
  if (next !== null) selectDrc(ctx, "violations", next);
}

/** `DIALOG_ERC::ExcludeMarker`: the same for the schematic's selected finding. */
export async function excludeMarkerErc(ctx: CheckerCtx): Promise<void> {
  const s = ctx.api.getState();
  const list = s.erc?.violations ?? [];
  const here = s.ercSelected;
  const v = here !== null ? list[here] : undefined;
  if (here === null || !v || !v.location || v.severity === "excluded") return;
  const filter = getCheckerView().erc.filter;
  const listed = listedIndexes(list, filter);
  if (!(await setErcExcluded(ctx, [v], true))) return;
  const next = filter.exclusions ? null : selectionAfterRowGone(listed, here);
  if (next !== null) selectErc(ctx, next);
}

// -------------------------------------------------------------------------------------------------------------------------- severities

/** `ID_SET_SEVERITY_TO_*` / the Ignored Tests radio menu on the board: `set_rule_severities`, and the report on screen follows. */
export async function setDrcSeverity(ctx: CheckerCtx, check: string, severity: RuleSeverity): Promise<boolean> {
  const { api, dispatch } = ctx;
  const before = api.getState();
  const table = before.board?.board_rules?.severities ?? {};
  if (!(await api.cmd({ op: "set_rule_severities", severities: severitiesAfter(DRC_CHECK_ITEMS, table, check, severity) }))) return false;
  dispatch({ type: "DRC_PATCH_SEVERITY", check, severity, description: drcTitle(check), version: await revisionAfter(before.drcVersion, before) });
  return true;
}

/** The same for the schematic: `set_erc_severities` over the table as the server has it. */
export async function setErcSeverity(ctx: CheckerCtx, check: string, severity: RuleSeverity): Promise<boolean> {
  const { api, dispatch } = ctx;
  const before = api.getState();
  const current = await fetchErcSeverities().then((r) => r.severities).catch(() => null);
  if (!current) return false;
  if (!(await api.cmd({ op: "set_erc_severities", severities: severitiesAfter(ERC_CHECK_ITEMS, current, check, severity) }))) return false;
  setErcView({ severities: null });
  dispatch({ type: "ERC_PATCH_SEVERITY", check, severity, description: ercTitle(check), version: await revisionAfter(before.ercVersion, before) });
  return true;
}

/** Read the schematic's severity table once, so a waived finding's menu can say what its check's severity is. */
export async function loadErcSeverities(): Promise<void> {
  try {
    const r = await fetchErcSeverities();
    if (r.ok) setErcView({ severities: r.severities });
  } catch {
    // The menu falls back to KiCad's default.
  }
}

// -------------------------------------------------------------------------------------------------------------------------- menu commands

/**
 * A marker menu (`rcMenu`) as the menu component draws it. `onPick` runs the chosen entry; `exclusionOk` is false for a marker that names no item to key an
 * exclusion on (its exclusion entries are then drawn disabled).
 */
export function toMenuEntries(spec: readonly RcMenuEntry[], onPick: (id: RcMenuId) => void, exclusionOk = true): MenuEntry[] {
  return spec.map((e) =>
    e.id === "separator"
      ? { label: "", separator: true, onSelect: () => undefined }
      : { label: e.label, hint: e.hint, disabled: e.disabled || (!exclusionOk && /exclu/.test(e.id)), onSelect: () => onPick(e.id as RcMenuId) }
  );
}

export interface MarkerTarget {
  domain: "drc" | "erc";
  /** The page of the DRC report the marker is on. */
  tab?: DrcTab;
  index: number;
}

/** "Edit violation severities..." (`ShowBoardSetupDialog( "Violation Severity" )`, `ShowSchematicSetupDialog( "Violation Severity" )`; "Edit pin-to-pin conflict map..." for that check): the setup dialog on that page. */
export function openSeveritySetup(ctx: CheckerCtx, domain: "drc" | "erc", pinMap = false): void {
  if (domain === "drc") {
    ctx.dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: "severities" });
    ctx.dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true });
  } else {
    setErcView({ setupPage: pinMap ? "pinmap" : "severities" });
    ctx.dispatch({ type: "SET_SCH_DIALOG", dialog: "setup" });
  }
}

/**
 * Run one entry of a marker's menu (`rcMenu`). `askComment` asks the user for the exclusion's comment (`WX_TEXT_ENTRY_DIALOG`, "Exclusion Comment") and
 * resolves to the text, or to null when they cancel.
 */
export async function runMarkerMenu(ctx: CheckerCtx, target: MarkerTarget, id: RcMenuId, askComment: (initial: string) => Promise<string | null>): Promise<void> {
  const { api } = ctx;
  const state = api.getState();
  if (target.domain === "drc") {
    const tab = target.tab ?? "violations";
    const v = drcListOf(state.drc, tab)[target.index];
    if (!v) return;
    switch (id) {
      case "exclude":
        await excludeDrc(ctx, [v]);
        return;
      case "exclude_comment": {
        const text = await askComment("");
        if (text !== null) await excludeDrc(ctx, [v], text);
        return;
      }
      case "edit_comment": {
        const text = await askComment(v.comment ?? "");
        if (text !== null) await excludeDrc(ctx, [v], text);
        return;
      }
      case "remove_exclusion":
        await restoreDrc(ctx, [v]);
        return;
      case "exclude_type":
        await excludeDrcOfCheck(ctx, v.type, true);
        return;
      case "remove_exclusion_type":
        await excludeDrcOfCheck(ctx, v.type, false);
        return;
      case "edit_severities":
        openSeveritySetup(ctx, "drc");
        return;
      case "show_in_dialog":
        showDrcInDialog(ctx, tab, target.index);
        return;
      default: {
        const severity = menuSeverity(id);
        if (severity) await setDrcSeverity(ctx, v.type, severity);
      }
    }
    return;
  }
  const v = state.erc?.violations[target.index];
  if (!v) return;
  switch (id) {
    case "exclude":
      await setErcExcluded(ctx, [v], true);
      return;
    case "remove_exclusion":
      await setErcExcluded(ctx, [v], false);
      return;
    case "exclude_type":
      await excludeErcOfCheck(ctx, v.check, true);
      return;
    case "remove_exclusion_type":
      await excludeErcOfCheck(ctx, v.check, false);
      return;
    case "edit_severities":
      openSeveritySetup(ctx, "erc", isPinMapCheck(v.check));
      return;
    case "show_in_dialog":
      showErcInDialog(ctx, target.index);
      return;
    default: {
      const severity = menuSeverity(id);
      if (severity) await setErcSeverity(ctx, v.check, severity);
    }
  }
}
