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

/**
 * The settings the dialog last held, kept between openings as KiCad keeps them (`~DIALOG_ANNOTATE` writes them back): Annotate reopens with them, and placing,
 * pasting and Import Sheet number symbols the way they say. Two homes, as in KiCad (UI-LEFT S14): the scope, "Recurse into subsheets", keep / reset and "Regroup
 * symbol units" are the user's (`EESCHEMA_SETTINGS::m_AnnotatePanel`, eeschema.json: this browser's storage here); the order, the numbering and the start number are
 * the project's (`SCHEMATIC_SETTINGS::m_Annotate*`: Schematic Setup > Annotation), read from the schematic as it loads ([`adoptProjectAnnotation`]) and stored back by
 * the dialog's close (`set_annotation_settings`).
 */
const PANEL_KEY = "eda.sch.annotatePanel";

function loadPanel(): AnnotateSettings {
  const base = defaultAnnotateSettings();
  try {
    const raw = globalThis.localStorage?.getItem(PANEL_KEY);
    if (!raw) return base;
    const v = JSON.parse(raw) as Partial<AnnotateSettings>;
    const scope = v.scope === "all" || v.scope === "current_sheet" || v.scope === "selection" ? v.scope : base.scope;
    return { ...base, scope, recursive: v.recursive ?? base.recursive, reset: v.reset ?? base.reset, regroupUnits: v.regroupUnits ?? base.regroupUnits };
  } catch {
    return base;
  }
}

let remembered: AnnotateSettings | null = null;

export function rememberedAnnotateSettings(): AnnotateSettings {
  if (!remembered) remembered = loadPanel();
  return remembered;
}

export function rememberAnnotateSettings(settings: AnnotateSettings): void {
  remembered = settings;
  try {
    globalThis.localStorage?.setItem(PANEL_KEY, JSON.stringify({ scope: settings.scope, recursive: settings.recursive, reset: settings.reset, regroupUnits: settings.regroupUnits }));
  } catch {
    // no storage: the panel's choices last the session
  }
}

/** The project's order, numbering and start number (`SCHEMATIC_SETTINGS::m_AnnotateSortOrder` / `m_AnnotateMethod` / `m_AnnotateStartNum`; null is KiCad's defaults). */
export function adoptProjectAnnotation(a: { sort_order: number; method: number; start_num: number } | null | undefined): void {
  const s = rememberedAnnotateSettings();
  const sort = a?.sort_order === 1 ? "y" : "x";
  const algo = a?.method === 1 ? "sheet_x100" : a?.method === 2 ? "sheet_x1000" : "first_free";
  remembered = { ...s, sort, algo, start: String(a?.start_num ?? 0) };
}

/**
 * `GetStartNumber()`: `(int) EDA_UNIT_UTILS::UI::ValueFromString( m_textNumberAfter->GetValue() )`, the field read as a number and rounded (`KiROUND`), so "9.6" is 10;
 * what is not a number counts from 0. A negative start is no start here (KiCad keeps the sign; the numbering takes no negative number).
 */
export function startNumber(text: string): number {
  const n = Number.parseFloat(text.trim());
  return Number.isFinite(n) && n > 0 ? Math.round(n) : 0;
}

/** What `paste_sch` is told to number by (r2 D-C3): `SCH_EDITOR_CONTROL::Paste` reads Schematic Setup's `m_AnnotateMethod`, `m_AnnotateStartNum` and
 *  `m_AnnotateSortOrder`, the settings the Annotate dialog last saved. */
export function pasteNumbering(s: Pick<AnnotateSettings, "sort" | "algo" | "start">): { algo: AnnotateSettings["algo"]; start: number; order: "by_x" | "by_y" } {
  return { algo: s.algo, start: startNumber(s.start), order: s.sort === "x" ? "by_x" : "by_y" };
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

/**
 * The scope the dialog starts in: `DIALOG_ANNOTATE::TransferDataToWindow` restores `m_AnnotatePanel.scope` as it was, whatever is selected (r2 D-A4). A
 * Selection scope with nothing selected annotates nothing (`getInferredSymbols` finds no symbol).
 */
export function scopeOnOpen(remembered: AnnotateScopeId, _selection: AnnotateSelection): AnnotateScopeId {
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
