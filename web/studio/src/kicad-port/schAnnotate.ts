// Annotate Schematic -- the pure parts of eeschema/dialogs/dialog_annotate.cpp (DIALOG_ANNOTATE) at 8303b2ad: the dialog's settings and their
// defaults (`EESCHEMA_SETTINGS::m_AnnotatePanel`, `SCHEMATIC_SETTINGS::m_Annotate*`), and the command and the report request they make.
// The numbering itself is the server's (crates/model/src/annotate.rs, a port of sch_reference_list.cpp).
import type { Cmd } from "../api/types";
import type { AnnotateOptions } from "../api/hierarchyTypes";

export type AnnotateScopeId = "all" | "current_sheet" | "selection";

/** What the dialog's controls hold. */
export interface AnnotateSettings {
  /** Scope: Entire schematic / Current sheet only / Selection. */
  scope: AnnotateScopeId;
  /** "Recurse into subsheets" (`m_AnnotatePanel.recursive`). */
  recursive: boolean;
  /** Options: "Reset existing annotations" (`m_AnnotatePanel.options >= 1`); false is "Keep existing annotations". */
  reset: boolean;
  /** "Regroup symbol units" (`m_AnnotatePanel.regroup_units`), only with a reset. */
  regroupUnits: boolean;
  /** Order: "Sort symbols by X position" (`SORT_BY_X_POSITION`, the default) or "by Y position". */
  sort: "x" | "y";
  /** Numbering: "Use first free number after", or "First free after sheet number X 100" / "X 1000". */
  algo: "first_free" | "sheet_x100" | "sheet_x1000";
  /** The text of the number the first-free search starts after (`m_AnnotateStartNum`, 0). */
  start: string;
}

/** `m_AnnotatePanel` and `SCHEMATIC_SETTINGS` as they start: the whole schematic, recursive, keep, sorted by X, first free after 0. */
export function defaultAnnotateSettings(): AnnotateSettings {
  return { scope: "all", recursive: true, reset: false, regroupUnits: false, sort: "x", algo: "first_free", start: "0" };
}

/** `GetStartNumber()`: the field read as a whole number; what is not one counts from 0, and a negative start is no start. */
export function startNumber(text: string): number {
  const n = Number.parseInt(text.trim(), 10);
  return Number.isFinite(n) && n > 0 ? n : 0;
}

/** What the canvas has selected, in the two kinds the Selection scope takes (`getInferredSymbols`, and the sheets when recursive). */
export interface AnnotateSelection {
  /** References of the selected symbols (a symbol's reference field counts as the symbol). */
  symbols: readonly string[];
  /** Ids of the selected sheet symbols of the sheet in view. */
  sheets: readonly string[];
}

/** The scope the dialog starts in when it is opened with something selected: the choice it had, but the selection when that is what is there. */
export function scopeOnOpen(remembered: AnnotateScopeId, selection: AnnotateSelection): AnnotateScopeId {
  const hasSelection = selection.symbols.length > 0 || selection.sheets.length > 0;
  if (remembered === "selection" && !hasSelection) return "all";
  return remembered;
}

function optionsOf(s: AnnotateSettings, selection: AnnotateSelection): AnnotateOptions {
  const o: AnnotateOptions = {
    scope: s.scope,
    algo: s.algo,
    start_number: startNumber(s.start),
    recursive: s.recursive,
    regroup_units: s.reset && s.regroupUnits,
  };
  if (s.scope === "selection") o.sheets = [...selection.sheets];
  return o;
}

/** The fields of `Cmd::Annotate` and of the report request for these settings. */
export function annotateRequest(s: AnnotateSettings, selection: AnnotateSelection): { reset_existing: boolean; order: "y_then_x" | "x_then_y"; ids?: string[]; options: AnnotateOptions } {
  const req: { reset_existing: boolean; order: "y_then_x" | "x_then_y"; ids?: string[]; options: AnnotateOptions } = {
    reset_existing: s.reset,
    // `SORT_BY_X_POSITION`: left to right (x first); `SORT_BY_Y_POSITION`: top to bottom
    order: s.sort === "x" ? "x_then_y" : "y_then_x",
    options: optionsOf(s, selection),
  };
  if (s.scope === "selection") req.ids = [...selection.symbols];
  return req;
}

/** The command that annotates. */
export function annotateCmd(s: AnnotateSettings, selection: AnnotateSelection): Cmd {
  return { op: "annotate", ...annotateRequest(s, selection) };
}
