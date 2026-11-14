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
// settings file, so both are this browser's (localStorage, a per-viewer convenience). The DRC dialog's two run options (Refill, Parity) are kept with its
// boxes, as KiCad 25ec2335's `SaveControlState` keeps every check box of the dialog. The ERC dialog's Show boxes start as `dialog_erc_base.fbp` has them --
// none ticked -- and are kept the same way, and its options menu (`DIALOG_ERC::OnMenu`) is the user's `EESCHEMA_SETTINGS::m_ERCDialog`.
import { useSyncExternalStore } from "react";
import { DRC_MENU_DEFAULTS, DRC_RUN_START, DRC_SHOW_START, ERC_MENU_DEFAULTS, ERC_SHOW_START, savedDrcControls, savedErcControls, type DrcMenuOptions, type DrcRunControls, type ErcMenuOptions, type RcFilter } from "../kicad-port/rcItems";
import type { RuleSeverity } from "../api/types";

/** `m_Notebook`'s pages in DIALOG_DRC (`dialog_drc_base.fbp`), plus the Lint page this app adds. */
/** The pages of Schematic Setup (components/SchematicSetupDialog.tsx), which a marker menu or an action can open it on. */
export type SchSetupPage = "formatting" | "annotation" | "field_templates" | "bom_presets" | "severities" | "symbol_parity" | "pinmap" | "netclasses" | "bus_aliases" | "text_variables" | "embedded_files";

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
    /** "Refill all zones before performing DRC" and "Test for parity between PCB and schematic". */
    run: DrcRunControls;
  };
  erc: {
    tab: ErcTab;
    filter: RcFilter;
    /** The schematic's severity table as last read (`GET /api/sch/erc_severities`): what the marker menu needs to say a check's severity when the finding is waived. */
    severities: Record<string, RuleSeverity> | null;
    /** The page Schematic Setup opens on (a marker menu's "Edit violation severities..." / "Edit pin-to-pin conflict map..." / "Edit connection grid spacing..."); null = the first. */
    setupPage: SchSetupPage | null;
    /** The options menu (the gear button). */
    options: ErcMenuOptions;
  };
}

export const DEFAULT_CHECKER_VIEW: CheckerView = {
  drc: { tab: "violations", filter: DRC_SHOW_START, unconnectedSelected: null, paritySelected: null, options: DRC_MENU_DEFAULTS, run: DRC_RUN_START },
  erc: { tab: "erc", filter: ERC_SHOW_START, severities: null, setupPage: null, options: ERC_MENU_DEFAULTS },
};

/** Where the DRC and ERC dialogs' kept controls live in this browser. */
const DRC_STORAGE_KEY = "eda.drcDialog";
const ERC_STORAGE_KEY = "eda.ercDialog";

function loadSaved(key: string): string | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage.getItem(key);
  } catch {
    return null;
  }
}

function store(key: string, value: unknown): void {
  try {
    if (typeof localStorage !== "undefined") localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // a private window or blocked storage: the controls last for this page only
  }
}

let view: CheckerView = {
  ...DEFAULT_CHECKER_VIEW,
  drc: { ...DEFAULT_CHECKER_VIEW.drc, ...savedDrcControls(loadSaved(DRC_STORAGE_KEY)) },
  erc: { ...DEFAULT_CHECKER_VIEW.erc, ...savedErcControls(loadSaved(ERC_STORAGE_KEY)) },
};
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
  if (patch.filter || patch.options || patch.run) store(DRC_STORAGE_KEY, { filter: view.drc.filter, options: view.drc.options, run: view.drc.run });
}

/** One entry of the DRC dialog's options menu toggled (`DIALOG_DRC::OnMenu`), kept for the next session (`PCBNEW_SETTINGS::m_DRCDialog`). */
export function setDrcOptions(patch: Partial<DrcMenuOptions>): void {
  setDrcView({ options: { ...view.drc.options, ...patch } });
}

/** The DRC dialog's Refill or Parity box changed, kept for the next opening and session (`SaveControlState`). */
export function setDrcRun(patch: Partial<DrcRunControls>): void {
  setDrcView({ run: { ...view.drc.run, ...patch } });
}

export function setErcView(patch: Partial<CheckerView["erc"]>): void {
  publish({ ...view, erc: { ...view.erc, ...patch } });
  if (patch.filter || patch.options) store(ERC_STORAGE_KEY, { filter: view.erc.filter, options: view.erc.options });
}

/** One entry of the ERC dialog's options menu toggled (`DIALOG_ERC::OnMenu`), kept for the next session (`EESCHEMA_SETTINGS::m_ERCDialog`). */
export function setErcOptions(patch: Partial<ErcMenuOptions>): void {
  setErcView({ options: { ...view.erc.options, ...patch } });
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
