// Annotate Schematic -- the pure parts of eeschema/dialogs/dialog_annotate.cpp (DIALOG_ANNOTATE) at 8303b2ad: the dialog's settings and their
// defaults (`EESCHEMA_SETTINGS::m_AnnotatePanel`, `SCHEMATIC_SETTINGS::m_Annotate*`), and the command and the report request they make.
// The numbering itself is the server's (crates/model/src/annotate.rs, a port of sch_reference_list.cpp).
import type { Cmd, Schematic } from "../api/types";
import { leafIds } from "./schGroups";
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

/** The settings the dialog last held, kept between openings as KiCad keeps them (`~DIALOG_ANNOTATE` writes them back): Annotate reopens with them, and Import Sheet numbers a new sheet's symbols the way they say. */
let remembered: AnnotateSettings = defaultAnnotateSettings();

export function rememberedAnnotateSettings(): AnnotateSettings {
  return remembered;
}

export function rememberAnnotateSettings(settings: AnnotateSettings): void {
  remembered = settings;
}

/**
 * `GetStartNumber()`: `(int) EDA_UNIT_UTILS::UI::ValueFromString( m_textNumberAfter->GetValue() )`, the field read as a number and rounded (`KiROUND`), so "9.6" is 10;
 * what is not a number counts from 0. A negative start is no start here (KiCad keeps the sign; the numbering takes no negative number).
 */
export function startNumber(text: string): number {
  const n = Number.parseFloat(text.trim());
  return Number.isFinite(n) && n > 0 ? Math.round(n) : 0;
}

/** The selection Annotate's "Selection" scope numbers (`annotate.cpp::getInferredSymbols`): a selected group stands for every symbol below it
 *  (`group->RunOnChildren( .., RECURSE_MODE::RECURSE )`), then the symbols (`isSymbol`) and the sheets of the sheet in view. */
export function annotateSelectionOf(sch: Schematic | null | undefined, selected: readonly string[], isSymbol: (id: string) => boolean): AnnotateSelection {
  const leaves = leafIds(sch, selected);
  return {
    symbols: [...new Set(leaves.filter(isSymbol))],
    // `AnnotateSymbols` takes the sheets from the selected items themselves (`for( EDA_ITEM* item : selection ) if( item->Type() == SCH_SHEET_T )`), not from
    // inside a selected group (r3 D3-L1)
    sheets: selected.filter((id) => sch?.sheets.some((s) => s.id === id) ?? false),
  };
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
