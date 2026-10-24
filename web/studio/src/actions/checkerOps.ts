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
import { fetchErcSeverities, fetchVersion, postSavedReport } from "../api/client";
import type { Cmd, DrcReport, DrcViolation, ErcViolation, RuleSeverity, SavedReportRequest } from "../api/types";
import { itemKind } from "../kicad-port/pcbItems";
import { openPropertiesFor } from "../components/canvas/properties";
import { inspectViolation } from "./inspectActions";
import drcChecks from "../kicad/drc_checks.json";
import ercChecks from "../kicad/erc_checks.json";
import { revisionAfterOwnEdit } from "../kicad-port/checkRevision";
import { canExclude, drcExclusionSpec, drcFixEntry, drcInspectEntry, drcKey, ercFixEntry, ercInspectEntry, ercKey, listedIndexes, menuSeverity, ofCheck, ofRule, selectionAfterRowGone, severitiesAfter, stepListed, violatingRule, type MarkerEntry, type RcMenuContext, type RcMenuEntry, type RcMenuId } from "../kicad-port/rcItems";
import { fitTransform } from "../kicad-port/view";
import { drcMarkerObject, objectOn } from "../kicad-port/appearance";
import { DRC_MARKER_HIT_NM, nearestMarker } from "../kicad-port/markerHit";
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

/**
 * Everything the marker menu of a DRC violation depends on (`OnDRCItemRClick`): its check, whether it is waived, the custom rule it broke, and the Inspect and Fix entries its check has --
 * the dialog's menu and the canvas marker's are built from the same one.
 */
export function drcMenuContext(state: Pick<StudioState, "board">, v: DrcViolation, onCanvas = false): RcMenuContext {
  const main = v.items[0]?.id ?? null;
  const kind = main && state.board ? itemKind(state.board, main) : null;
  return {
    domain: "drc",
    title: drcTitle(v.type),
    excluded: v.excluded === true,
    severity: drcSettingSeverity(state, v.type),
    rule: violatingRule(v.description),
    inspect: v.items.length > 0 ? drcInspectEntry(v.type) : null,
    fix: v.items.length > 0 ? drcFixEntry(v.type, kind) : null,
    ...(onCanvas ? { onCanvas } : {}),
  };
}

/** The same for an ERC finding (`OnERCItemRClick`); `endpoint_off_grid` also offers Schematic Setup's connection grid. */
export function ercMenuContext(v: ErcViolation, onCanvas = false): RcMenuContext {
  return {
    domain: "erc",
    title: ercTitle(v.check),
    excluded: v.severity === "excluded",
    severity: ercSettingSeverity(v),
    pinMap: isPinMapCheck(v.check),
    inspect: ercInspectEntry(v.check),
    fix: ercFixEntry(v.check),
    connectionGrid: v.check === "endpoint_off_grid",
    ...(onCanvas ? { onCanvas } : {}),
  };
}

// -------------------------------------------------------------------------------------------------------------------------- running an action by id

let actionRunner: ((id: string, param?: unknown) => void) | null = null;

/** Installed by `useEdaTestHook` next to the global hotkeys: the action runner (`useActionRunner`'s `run`), so a marker's Fix entry can run an action by its id from outside React. */
export function setCheckerRunner(run: ((id: string, param?: unknown) => void) | null): void {
  actionRunner = run;
}

/** How long a Fix entry waits for the selection it just made to reach the runner, which reads the selection of the render it was made in. */
const SELECT_THEN_RUN_MS = 80;

// -------------------------------------------------------------------------------------------------------------------------- selecting

/** Our id for a violation's item -> the id the canvas selects by: a pad (`REF.PAD`) selects its part, a track segment (`id#n`) its track; the Edge.Cuts outline is nothing to select. */
const baseRef = (id: string) => id.split("#")[0]!.split(".")[0]!;

/** A close-up needs some room around a single point: 2 mm either side. */
const FRAME_PAD_NM = 2_000_000;

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
 * What a click on a DRC row does to the canvas, whichever list the row is in (`DIALOG_DRC::OnDRCItemSelected`): select the items the violation names on the
 * board and frame them (the same click the real dialog turns into `FocusOnItems`). `showPcb`: also bring the PCB tab up, for a click that may come from
 * another editor.
 */
export function frameDrc(ctx: CheckerCtx, v: Pick<DrcViolation, "items"> | undefined, showPcb = false): void {
  const { dispatch } = ctx;
  if (v) {
    const refs = v.items.flatMap((it) => (it.id && it.id !== "outline" ? [baseRef(it.id)] : []));
    dispatch({ type: "SET_SELECTION", refs });
    dispatch({ type: "SET_HOT", refs });
    const rect = canvasRect();
    if (rect && rect.width >= 50 && rect.height >= 50 && v.items.length > 0) {
      const xs = v.items.map((it) => it.pos[0]);
      const ys = v.items.map((it) => it.pos[1]);
      const bounds = { minX: Math.min(...xs) - FRAME_PAD_NM, minY: Math.min(...ys) - FRAME_PAD_NM, maxX: Math.max(...xs) + FRAME_PAD_NM, maxY: Math.max(...ys) + FRAME_PAD_NM };
      dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height, 60) });
    }
  }
  if (showPcb) dispatch({ type: "SET_TAB", tab: "pcb" });
}

/** Select a row of one of the DRC dialog's marker pages, and its violation on the board ([`frameDrc`]). */
export function selectDrc(ctx: CheckerCtx, tab: DrcTab, index: number, showPcb = false): void {
  const { api, dispatch } = ctx;
  if (tab === "violations") dispatch({ type: "SET_DRC_SELECTED", index });
  else {
    dispatch({ type: "SET_DRC_SELECTED", index: null });
    setDrcView(tab === "unconnected" ? { unconnectedSelected: index } : { paritySelected: index });
  }
  frameDrc(ctx, drcListOf(api.getState().drc, tab)[index], showPcb);
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
      dispatch({ type: "SET_SCHEMATIC_VIEW", view: fitTransform({ minX: x - FRAME_PAD_NM, minY: y - FRAME_PAD_NM, maxX: x + FRAME_PAD_NM, maxY: y + FRAME_PAD_NM }, rect.width, rect.height, 60) });
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

/**
 * `ERC` twins: the exclusion is a `(check, location)` key with a comment and its marker (`ErcViolation.marker`, which the derived project lists
 * for kicad-cli), applied to the report on screen. `comment` is `SCH_MARKER::SetExcluded`'s; `edit` is "Edit exclusion comment...": the findings already
 * waived get the new comment instead of being skipped.
 */
export async function setErcExcluded(ctx: CheckerCtx, violations: readonly ErcViolation[], excluded: boolean, comment = "", edit = false): Promise<boolean> {
  const { api, dispatch } = ctx;
  const wanted = (v: ErcViolation) => (edit ? v.severity === "excluded" : excluded ? v.severity !== "excluded" : v.severity === "excluded");
  const picked = violations.flatMap((v) => {
    const k = wanted(v) ? ercKey(v) : null;
    return k ? [{ k, marker: v.marker }] : [];
  });
  if (picked.length === 0) return false;
  const before = api.getState();
  const cmds: Cmd[] = picked.map(({ k, marker }) =>
    excluded ? { op: "add_erc_exclusion", check: k.check, location: k.location, comment, ...(marker ? { marker } : {}) } : { op: "delete_erc_exclusion", check: k.check, location: k.location }
  );
  if (!(await api.cmdBatch(cmds))) return false;
  const version = await revisionAfter(before.ercVersion, before);
  for (const { k } of picked) dispatch({ type: "ERC_MARK_EXCLUDED", check: k.check, location: k.location, excluded, comment, version });
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

/** Waive (or restore) every violation of one custom rule on every page of the report: "Exclude all violations of rule '...'" (`ID_ADD_EXCLUSION_ALL`, `ID_REMOVE_EXCLUSION_ALL`). */
export async function excludeDrcOfRule(ctx: CheckerCtx, rule: string, excluded: boolean): Promise<boolean> {
  const report = ctx.api.getState().drc;
  if (!report) return false;
  const all = [...report.violations, ...(report.unconnected_items ?? []), ...(report.schematic_parity ?? [])];
  const of = ofRule(all, rule, !excluded);
  return excluded ? excludeDrc(ctx, of) : restoreDrc(ctx, of);
}

// -------------------------------------------------------------------------------------------------------------------------- Delete Marker

/** Where the selection goes when a row is deleted: the row that followed it among the listed ones, else the one before (`RC_TREE_MODEL::DeleteItems`), as an index of the list after the deletion. */
function afterDelete(listed: readonly number[], gone: number): { old: number | null; next: number | null } {
  const old = selectionAfterRowGone(listed, gone);
  return { old, next: old === null ? null : old > gone ? old - 1 : old };
}

/**
 * Delete Marker on the DRC dialog (`DIALOG_DRC::OnDeleteOneClick`, `RC_TREE_MODEL::DeleteCurrentItem( true )`): the selected marker of the page that is up leaves the list and the
 * board. An excluded marker takes its exclusion with it: `BOARD::RecordDRCExclusions` -- which the next DRC run and the next save make -- keeps the exclusions of the markers that
 * are left, so a deleted one is not (`delete_drc_exclusions`) and the violation is found again by the next run, not excluded. A violation that is not excluded only leaves the list.
 */
export async function deleteMarkerDrc(ctx: CheckerCtx): Promise<boolean> {
  const { api, dispatch } = ctx;
  const s = api.getState();
  const { tab, filter } = getCheckerView().drc;
  if (tab !== "violations" && tab !== "unconnected" && tab !== "parity") return false;
  const list = drcListOf(s.drc, tab);
  const here = drcSelectedOf(s, tab);
  const v = here !== null ? list[here] : undefined;
  if (here === null || !v) return false;
  const { old, next } = afterDelete(listedIndexes(list, filter), here);
  let version: string | null | undefined;
  if (v.excluded === true && canExclude(v)) {
    if (!(await api.cmd({ op: "delete_drc_exclusions", exclusions: [drcKey(v)] }))) return false;
    version = await revisionAfter(s.drcVersion, s);
  }
  dispatch({ type: "DRC_DELETE_MARKER", page: tab, index: here, select: next, ...(version !== undefined ? { version } : {}) });
  if (tab !== "violations") setDrcView(tab === "unconnected" ? { unconnectedSelected: next } : { paritySelected: next });
  if (old !== null) frameDrc(ctx, list[old]);
  return true;
}

/**
 * Delete All Markers on the DRC dialog (`OnDeleteAllClick`): the errors and warnings of every page, and the exclusions too when `includeExclusions` ("Errors, Warnings and
 * Exclusions"): with no marker left, `RecordDRCExclusions` keeps none (`clear_drc_exclusions`, one undo step).
 */
export async function deleteAllMarkersDrc(ctx: CheckerCtx, includeExclusions: boolean): Promise<boolean> {
  const { api, dispatch } = ctx;
  const s = api.getState();
  const report = s.drc;
  const excluded = [...(report?.violations ?? []), ...(report?.unconnected_items ?? []), ...(report?.schematic_parity ?? [])].some((v) => v.excluded === true);
  let version: string | null | undefined;
  if (includeExclusions && excluded) {
    if (!(await api.cmd({ op: "clear_drc_exclusions" }))) return false;
    version = await revisionAfter(s.drcVersion, s);
  }
  dispatch({ type: "DRC_DELETE_ALL", includeExclusions, ...(version !== undefined ? { version } : {}) });
  setDrcView({ unconnectedSelected: null, paritySelected: null });
  return true;
}

/**
 * Delete Marker on the ERC dialog (`DIALOG_ERC::OnDeleteOneClick`). An excluded marker takes its exclusion with it: KiCad records the markers that are left
 * (`RecordERCExclusions`, when ERC is run and when the project is saved), so a deleted one is not recorded -- the finding is reported again at the next run, and not
 * excluded. A finding that is not excluded only leaves the list; the next run finds it again.
 */
export async function deleteMarkerErc(ctx: CheckerCtx): Promise<boolean> {
  const { api, dispatch } = ctx;
  const s = api.getState();
  const list = s.erc?.violations ?? [];
  const here = s.ercSelected;
  const v = here !== null ? list[here] : undefined;
  if (here === null || !v) return false;
  const { old, next } = afterDelete(listedIndexes(list, getCheckerView().erc.filter), here);
  const key = v.severity === "excluded" ? ercKey(v) : null;
  let version: string | null | undefined;
  if (key) {
    if (!(await api.cmd({ op: "delete_erc_exclusion", check: key.check, location: key.location }))) return false;
    version = await revisionAfter(s.ercVersion, s);
  }
  dispatch({ type: "ERC_DELETE_MARKER", index: here, select: next, ...(version !== undefined ? { version } : {}) });
  if (old !== null) frameErc(ctx, list[old]);
  return true;
}

/** Delete All Markers on the ERC dialog (`OnDeleteAllClick`); with the exclusions, they are forgotten too (one undo step). */
export async function deleteAllMarkersErc(ctx: CheckerCtx, includeExclusions: boolean): Promise<boolean> {
  const { api, dispatch } = ctx;
  const s = api.getState();
  let version: string | null | undefined;
  if (includeExclusions && (s.erc?.violations ?? []).some((v) => v.severity === "excluded")) {
    if (!(await api.cmd({ op: "clear_erc_exclusions" }))) return false;
    version = await revisionAfter(s.ercVersion, s);
  }
  dispatch({ type: "ERC_DELETE_ALL", includeExclusions, ...(version !== undefined ? { version } : {}) });
  return true;
}

// -------------------------------------------------------------------------------------------------------------------------- Save report

/** Hand a text to the browser as a file to save (the page's "file dialog"). */
export function downloadText(fileName: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = fileName;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

/**
 * The dialogs' "Save..." (`DIALOG_ERC::OnSaveReport`, `DIALOG_DRC::OnSaveReport`): kicad-cli writes the report -- text (`.rpt`) or JSON -- for the severities the Show boxes
 * list, in the editor's units, and the page saves it as `ERC.rpt` / `DRC.json`. A fresh run of kicad-cli, as the report is its own: a marker deleted from the list
 * is in it, and the exclusions are the derived project's. Resolves to whether a file was saved; the reason is a toast.
 */
export async function saveReport(ctx: CheckerCtx, domain: "erc" | "drc", format: "report" | "json"): Promise<boolean> {
  const { api, dispatch } = ctx;
  const state = api.getState();
  const filter = getCheckerView()[domain].filter;
  const request: SavedReportRequest = { format, errors: filter.errors, warnings: filter.warnings, exclusions: filter.exclusions, units: state.units };
  if (domain === "drc") {
    request.refill_zones = state.drcRefillZones;
    request.schematic_parity = state.drcParity;
  }
  try {
    const reply = await postSavedReport(domain, request);
    if (!reply.ok || reply.report === undefined) {
      dispatch({ type: "TOAST", message: reply.message ?? "kicad-cli could not write the report.", kind: "error" });
      return false;
    }
    downloadText(reply.file_name ?? (domain === "erc" ? "ERC" : "DRC") + (format === "json" ? ".json" : ".rpt"), reply.report);
    dispatch({ type: "TOAST", message: `Report file '${reply.file_name ?? ""}' created.`, kind: "info" });
    return true;
  } catch (e) {
    dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });
    return false;
  }
}

// -------------------------------------------------------------------------------------------------------------------------- selecting a marker on the canvas

/** The DRC marker on the board at (x, y) µm that is drawn -- its kind is switched on in the Objects tab, or it is the one the dialog has selected (components/canvas/painter.ts `drawDrcMarkers`) -- or -1. */
export function drcMarkerAt(state: Pick<StudioState, "drc" | "layerVisible" | "drcSelected">, x: number, y: number): number {
  const list = state.drc?.violations ?? [];
  return nearestMarker(
    list.map((v, i) => (i === state.drcSelected || objectOn(state.layerVisible, drcMarkerObject(v)) ? (v.items[0]?.pos ?? null) : null)),
    x,
    y,
    DRC_MARKER_HIT_NM
  );
}

/**
 * A left click on a DRC marker on the board (`PCB_SELECTION_TOOL::selectPoint` takes a `PCB_MARKER` like any item, and `DRC_TOOL::CrossProbe` then selects its row in
 * the dialog when that is shown): the marker is the selection -- the items that were selected are not -- drawn highlighted, and the dialog's Violations page has it.
 */
export function selectMarkerDrc(ctx: CheckerCtx, index: number): void {
  ctx.dispatch({ type: "CLEAR_SELECTION" });
  ctx.dispatch({ type: "SET_DRC_SELECTED", index });
  if (ctx.api.getState().drcDialogOpen) setDrcView({ tab: "violations" });
}

/** The same on the schematic (`SCH_INSPECTION_TOOL::CrossProbe`). */
export function selectMarkerErc(ctx: CheckerCtx, index: number): void {
  ctx.dispatch({ type: "CLEAR_SELECTION" });
  ctx.dispatch({ type: "SET_ERC_SELECTED", index });
  if (ctx.api.getState().ercDialogOpen) setErcView({ tab: "erc" });
}

// -------------------------------------------------------------------------------------------------------------------------- Inspect and Fix

/** What a marker's item is selected as before an action that works on the selection runs: a pad or a track segment stands for its footprint or its track, a field for its footprint. */
function selectableRef(id: string): string {
  return id.includes(":") ? id.split(":")[0]! : baseRef(id);
}

/** Select the item (when the action works on the selection), then run the action once the selection has reached the runner. */
function selectThenRun(ctx: CheckerCtx, tab: "pcb" | "schematic", id: string, select: string | null): void {
  ctx.dispatch({ type: "SET_TAB", tab });
  if (select) ctx.dispatch({ type: "SET_SELECTION", refs: [select] });
  setTimeout(() => actionRunner?.(id), select ? SELECT_THEN_RUN_MS : 0);
}

/** An Inspect or Fix entry of a DRC violation (`InspectDRCError`, `FixDRCError`). */
export function runDrcEntry(ctx: CheckerCtx, entry: MarkerEntry, v: DrcViolation): void {
  const { api, dispatch } = ctx;
  const c = entry.command;
  const main = v.items[0]?.id ?? null;
  switch (c.kind) {
    case "inspect_clearance":
      void inspectViolation(api, dispatch, "clearance", v);
      return;
    case "inspect_constraints":
      void inspectViolation(api, dispatch, "constraints", v);
      return;
    case "properties":
      // `OnEditItemRequest( item )`: the item's own properties dialog, on the board.
      if (!main) return;
      dispatch({ type: "SET_TAB", tab: "pcb" });
      dispatch({ type: "SET_SELECTION", refs: [selectableRef(main)] });
      openPropertiesFor(main, api, dispatch);
      return;
    case "action":
      selectThenRun(ctx, "pcb", c.id, c.select && main ? selectableRef(main) : null);
      return;
  }
}

/** An Inspect or Fix entry of an ERC finding (`InspectERCError`, `FixERCError`): the finding's main item is what the action works on. */
export function runErcEntry(ctx: CheckerCtx, entry: MarkerEntry, v: ErcViolation): void {
  const c = entry.command;
  if (c.kind !== "action") return;
  const main = v.items?.find((i) => i.id)?.id ?? v.location ?? null;
  selectThenRun(ctx, "schematic", c.id, c.select && main ? main : null);
}

/** "Edit connection grid spacing..." (`ShowSchematicSetupDialog( "Formatting" )`): Schematic Setup on its Formatting page. */
export function openConnectionGridSetup(ctx: CheckerCtx): void {
  setErcView({ setupPage: "formatting" });
  ctx.dispatch({ type: "SET_SCH_DIALOG", dialog: "setup" });
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
      case "exclude_rule":
      case "remove_exclusion_rule": {
        const rule = violatingRule(v.description);
        if (rule) await excludeDrcOfRule(ctx, rule, id === "exclude_rule");
        return;
      }
      case "inspect":
      case "fix": {
        const menu = drcMenuContext(state, v);
        const entry = id === "inspect" ? menu.inspect : menu.fix;
        if (entry) runDrcEntry(ctx, entry, v);
        return;
      }
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
    case "exclude_comment": {
      const text = await askComment("");
      if (text !== null) await setErcExcluded(ctx, [v], true, text);
      return;
    }
    case "edit_comment": {
      const text = await askComment(v.comment ?? "");
      if (text !== null) await setErcExcluded(ctx, [v], true, text, true);
      return;
    }
    case "remove_exclusion":
      await setErcExcluded(ctx, [v], false);
      return;
    case "exclude_type":
      await excludeErcOfCheck(ctx, v.check, true);
      return;
    case "remove_exclusion_type":
      await excludeErcOfCheck(ctx, v.check, false);
      return;
    case "inspect":
    case "fix": {
      const menu = ercMenuContext(v);
      const entry = id === "inspect" ? menu.inspect : menu.fix;
      if (entry) runErcEntry(ctx, entry, v);
      return;
    }
    case "connection_grid":
      openConnectionGridSetup(ctx);
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
