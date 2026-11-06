// What the Checker windows (the DRC and ERC dialogs) have on show, kept apart from the reports themselves: the notebook page, the "Show" boxes,
// and the row selected on the pages that are not the Violations page (that page's is `state.drcSelected` / `state.ercSelected`, which the canvas
// markers read too).
//
// It lives in a small external store, like `commonOptions.ts`, for one reason: Next / Previous / Exclude Marker (actions/commonCheckerActions.ts)
// step the list the dialog is showing -- `DIALOG_DRC::NextMarker` asks the notebook which page is up, `RC_TREE_MODEL::NextMarker` walks the rows the
// Show boxes let through -- and an action must read the same page and boxes the dialog draws. The page starts on Violations each time
// (`m_Notebook->SetSelection( 0 )`). The DRC dialog's Show boxes start as `dialog_drc_base.fbp` has them -- Errors and Warnings ticked, Exclusions and
// All not (R6-D9) -- and are kept from one opening to the next, and across sessions, as `DIALOG_SHIM::SaveControlState` keeps every control of a dialog
// in the common settings; the options menu's three entries (`DIALOG_DRC::OnMenu`) are the user's `PCBNEW_SETTINGS::m_DRCDialog`. The studio has no
// settings file, so both are this browser's (localStorage, a per-viewer convenience). The ERC dialog's boxes stay as they were (all shown).
import { useSyncExternalStore } from "react";
import { DRC_MENU_DEFAULTS, DRC_SHOW_START, SHOW_ALL, savedDrcControls, type DrcMenuOptions, type RcFilter } from "../kicad-port/rcItems";
import type { RuleSeverity } from "../api/types";

/** `m_Notebook`'s pages in DIALOG_DRC (`dialog_drc_base.fbp`), plus the Lint page this app adds. */
/** The pages of Schematic Setup (components/SchematicSetupDialog.tsx), which a marker menu or an action can open it on. */
export type SchSetupPage = "formatting" | "annotation" | "field_templates" | "bom_presets" | "severities" | "pinmap" | "netclasses" | "bus_aliases" | "text_variables" | "embedded_files";

export type DrcTab = "violations" | "unconnected" | "parity" | "ignored" | "lint";
/** DIALOG_ERC's pages ("Violations", "Ignored Tests") plus Lint. */
export type ErcTab = "erc" | "ignored" | "lint";

export interface CheckerView {
  drc: {
    tab: DrcTab;
    filter: RcFilter;
    /** The row selected on the Unconnected Items page (the Violations page's is `state.drcSelected`). */
    unconnectedSelected: number | null;
    /** The row selected on the Schematic Parity page. */
    paritySelected: number | null;
    /** The options menu (the gear button). */
    options: DrcMenuOptions;
  };
  erc: {
    tab: ErcTab;
    filter: RcFilter;
    /** The schematic's severity table as last read (`GET /api/sch/erc_severities`): what the marker menu needs to say a check's severity when the finding is waived. */
    severities: Record<string, RuleSeverity> | null;
    /** The page Schematic Setup opens on (a marker menu's "Edit violation severities..." / "Edit pin-to-pin conflict map..." / "Edit connection grid spacing..."); null = the first. */
    setupPage: SchSetupPage | null;
  };
}

export const DEFAULT_CHECKER_VIEW: CheckerView = {
  drc: { tab: "violations", filter: DRC_SHOW_START, unconnectedSelected: null, paritySelected: null, options: DRC_MENU_DEFAULTS },
  erc: { tab: "erc", filter: SHOW_ALL, severities: null, setupPage: null },
};

/** Where the DRC dialog's kept controls live in this browser. */
const DRC_STORAGE_KEY = "eda.drcDialog";

function loadDrcControls(): { filter: RcFilter; options: DrcMenuOptions } {
  try {
    return savedDrcControls(typeof localStorage === "undefined" ? null : localStorage.getItem(DRC_STORAGE_KEY));
  } catch {
    return savedDrcControls(null);
  }
}

function storeDrcControls(drc: CheckerView["drc"]): void {
  try {
    if (typeof localStorage !== "undefined") localStorage.setItem(DRC_STORAGE_KEY, JSON.stringify({ filter: drc.filter, options: drc.options }));
  } catch {
    // a private window or blocked storage: the controls last for this page only
  }
}

let view: CheckerView = { ...DEFAULT_CHECKER_VIEW, drc: { ...DEFAULT_CHECKER_VIEW.drc, ...loadDrcControls() } };
const listeners = new Set<() => void>();

function publish(next: CheckerView): void {
  view = next;
  for (const l of listeners) l();
}

export function getCheckerView(): CheckerView {
  return view;
}

export function setDrcView(patch: Partial<CheckerView["drc"]>): void {
  publish({ ...view, drc: { ...view.drc, ...patch } });
  if (patch.filter || patch.options) storeDrcControls(view.drc);
}

/** One entry of the DRC dialog's options menu toggled (`DIALOG_DRC::OnMenu`), kept for the next session (`PCBNEW_SETTINGS::m_DRCDialog`). */
export function setDrcOptions(patch: Partial<DrcMenuOptions>): void {
  setDrcView({ options: { ...view.drc.options, ...patch } });
}

export function setErcView(patch: Partial<CheckerView["erc"]>): void {
  publish({ ...view, erc: { ...view.erc, ...patch } });
}

/** Back to the start-of-session view (tests). */
export function resetCheckerView(): void {
  publish(DEFAULT_CHECKER_VIEW);
}

export function useCheckerView(): CheckerView {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => view,
    () => view
  );
}

// ---------------------------------------------------------------------------------------------------------------------- the comment prompt

/** `WX_TEXT_ENTRY_DIALOG( this, wxEmptyString, _( "Exclusion Comment" ), comment, true )`: one question at a time, answered with the text or null (cancel). */
export interface CommentRequest {
  initial: string;
  resolve: (text: string | null) => void;
}

let pending: CommentRequest | null = null;
const commentListeners = new Set<() => void>();

/** Ask for an exclusion's comment: resolves with what the user typed, or null when they cancel (or another question replaces this one). */
export function askExclusionComment(initial: string): Promise<string | null> {
  pending?.resolve(null);
  return new Promise((resolve) => {
    pending = { initial, resolve };
    for (const l of commentListeners) l();
  });
}

/** The prompt's answer; closes it. */
export function answerExclusionComment(text: string | null): void {
  const request = pending;
  pending = null;
  for (const l of commentListeners) l();
  request?.resolve(text);
}

export function useCommentRequest(): CommentRequest | null {
  return useSyncExternalStore(
    (l) => {
      commentListeners.add(l);
      return () => commentListeners.delete(l);
    },
    () => pending,
    () => pending
  );
}
