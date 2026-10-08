// What the Checker windows (the DRC and ERC dialogs) have on show, kept apart from the reports themselves: the notebook page, the "Show" boxes,
// and the row selected on the pages that are not the Violations page (that page's is `state.drcSelected` / `state.ercSelected`, which the canvas
// markers read too).
//
// It lives in a small external store, like `commonOptions.ts`, for one reason: Next / Previous / Exclude Marker (actions/commonCheckerActions.ts)
// step the list the dialog is showing -- `DIALOG_DRC::NextMarker` asks the notebook which page is up, `RC_TREE_MODEL::NextMarker` walks the rows the
// Show boxes let through -- and an action must read the same page and boxes the dialog draws. Not kept between sessions: KiCad's dialog starts
// each session on the Violations page with everything shown.
import { useSyncExternalStore } from "react";
import { SHOW_ALL, type RcFilter } from "../kicad-port/rcItems";
import type { RuleSeverity } from "../api/types";

/** `m_Notebook`'s pages in DIALOG_DRC (`dialog_drc_base.fbp`), plus the Lint page this app adds. */
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
  };
  erc: {
    tab: ErcTab;
    filter: RcFilter;
    /** The schematic's severity table as last read (`GET /api/sch/erc_severities`): what the marker menu needs to say a check's severity when the finding is waived. */
    severities: Record<string, RuleSeverity> | null;
    /** The page Schematic Setup opens on (a marker menu's "Edit violation severities..." / "Edit pin-to-pin conflict map..."); null = the first. */
    setupPage: "severities" | "pinmap" | null;
  };
}

export const DEFAULT_CHECKER_VIEW: CheckerView = {
  drc: { tab: "violations", filter: SHOW_ALL, unconnectedSelected: null, paritySelected: null },
  erc: { tab: "erc", filter: SHOW_ALL, severities: null, setupPage: null },
};

let view: CheckerView = DEFAULT_CHECKER_VIEW;
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
