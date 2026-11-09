// Change Symbols / Update Symbols -- `DIALOG_CHANGE_SYMBOLS` (eeschema/dialogs/dialog_change_symbols.cpp at 8303b2ad): which symbols a request
// matches (`isMatch`), which of them can take the new library symbol (`processSymbols`' checks), and what the change does to their reference and value.
//
// The reset options (field visibilities, sizes and styles, positions) are `reset_symbol_fields` in the change's own step ([`resetCmd`], [`changeStep`]): KiCad's
// `processSymbols` makes the change and the resets in one `SCH_COMMIT` (UI-LEFT-2 SS14; before, the reset was a second undo step after the change).
import type { Cmd } from "../api/types";

export type ChangeMode = "change" | "update";
export type MatchBy = "selection" | "all" | "reference" | "value" | "id";

export interface MatchSpec {
  by: MatchBy;
  reference: string;
  value: string;
  id: string;
}

/** One placed symbol (a unit of a reference), as the matching needs it. */
export interface SymbolRow {
  id: string;
  lib_id: string | null;
  unit: number;
  value: string | null;
  /** The reference the row's sheet path gives it (`GetRef( aInstance )`), when that is not `id` (a screen shown by several sheets). */
  reference?: string;
  /** The sheet path of the row (`/`-joined placement ids) and the screen it shows; absent: the sheet in view. */
  sheet?: string;
  file?: string;
}

/**
 * `processMatchingSymbols` over the hierarchy (`for( SCH_SHEET_PATH& instance : Hierarchy() )`, `isMatch( symbol, &instance )`): the symbols that match on any
 * sheet path, grouped by the screen they are on, each screen with the sheet path its verbs go to and the symbols' ids (each
 * once, in walk order). A symbol of a screen shown on two paths is one symbol: matched on either path, it is changed once (`std::map<SCH_SYMBOL*, ..>`),
 * through the first path it matched on.
 * `selected` are ids on the sheet in view (`inView`, a `/`-joined path).
 */
export function matchOverHierarchy(rows: readonly SymbolRow[], spec: MatchSpec, selected: ReadonlySet<string>, inView: string): Array<{ sheet: string; file: string; refs: string[] }> {
  const out: Array<{ sheet: string; file: string; refs: string[] }> = [];
  for (const s of rows) {
    const shown = s.reference ?? s.id;
    let hit: boolean;
    switch (spec.by) {
      case "all":
        hit = true;
        break;
      case "selection":
        hit = (s.sheet ?? "") === inView && selected.has(s.id);
        break;
      case "reference":
        hit = wildMatch(spec.reference, shown);
        break;
      case "value":
        hit = wildMatch(spec.value, s.value ?? "");
        break;
      case "id":
        hit = s.lib_id === spec.id;
        break;
    }
    if (!hit) continue;
    const file = s.file ?? "";
    let group = out.find((g) => g.file === file);
    if (!group) {
      group = { sheet: s.sheet ?? "", file, refs: [] };
      out.push(group);
    }
    if (!group.refs.includes(s.id)) group.refs.push(s.id);
  }
  return out;
}

/** `getSymbolReferences`: every reference a symbol has on the paths of the hierarchy (`GetInstances()`), space separated, for the report. */
export function referencesOf(rows: readonly SymbolRow[], file: string, id: string): string {
  const refs: string[] = [];
  for (const r of rows) if ((r.file ?? "") === file && r.id === id && !refs.includes(r.reference ?? r.id)) refs.push(r.reference ?? r.id);
  return refs.join(" ");
}

/** `WildCompareString( pattern, text, aCaseSensitive )`: `*` any run, `?` any one character. */
export function wildMatch(pattern: string, text: string, caseSensitive = false): boolean {
  const p = caseSensitive ? pattern : pattern.toLowerCase();
  const t = caseSensitive ? text : text.toLowerCase();
  // Iterative matcher with backtracking to the last `*`.
  let pi = 0;
  let ti = 0;
  let star = -1;
  let mark = 0;
  while (ti < t.length) {
    if (pi < p.length && (p[pi] === "?" || p[pi] === t[ti])) {
      pi++;
      ti++;
    } else if (pi < p.length && p[pi] === "*") {
      star = pi++;
      mark = ti;
    } else if (star >= 0) {
      pi = star + 1;
      ti = ++mark;
    } else return false;
  }
  while (pi < p.length && p[pi] === "*") pi++;
  return pi === p.length;
}

/** `isMatch` over the sheet: the references (each once, in sheet order) whose symbols match the request. */
export function matchReferences(symbols: readonly SymbolRow[], spec: MatchSpec, selected: ReadonlySet<string>): string[] {
  const hit = (s: SymbolRow): boolean => {
    switch (spec.by) {
      case "all":
        return true;
      case "selection":
        return selected.has(s.id);
      case "reference":
        return wildMatch(spec.reference, s.id);
      case "value":
        return wildMatch(spec.value, s.value ?? "");
      case "id":
        return s.lib_id === spec.id;
    }
  };
  const out: string[] = [];
  for (const s of symbols) if (hit(s) && !out.includes(s.id)) out.push(s.id);
  return out;
}

/** `UTIL::GetRefDesPrefix`: a reference without its number ("R12" -> "R", "#PWR03" -> "#PWR", "U?" -> "U"). */
export const refPrefix = (ref: string): string => ref.replace(/[0-9?]+$/, "");
/** `UTIL::GetRefDesNumber`: the number of a reference, or -1 when it has none ("R?"). */
export const refNumber = (ref: string): number => {
  const m = /([0-9]+)$/.exec(ref);
  return m ? Number(m[1]) : -1;
};

/** The reference a symbol gets when its library symbol's prefix changes: the new prefix, the old number kept ("R12" under "C" -> "C12"; "R?" -> "C?"). */
export function reprefix(ref: string, prefix: string): string {
  const n = refNumber(ref);
  return n >= 0 ? `${prefix}${n}` : `${prefix}?`;
}

/** What the new library symbol offers: whether it exists, its unit count, its reference prefix, its Value text. */
export interface LibInfo {
  found: boolean;
  unitCount: number;
  prefix: string;
  value: string;
}

export interface ChangeOptions {
  newLibId: string;
  updateReference: boolean;
  updateValue: boolean;
  /** `m_resetEmptyFields`: reset a field even when the library's own is empty. */
  resetEmpty: boolean;
  /** How the report names a symbol (`getSymbolReferences`: its references on every sheet path); its id when absent. */
  shown?: (ref: string) => string;
}

export interface ChangePlan {
  cmds: Cmd[];
  /** The references `processSymbols` went on to (every one not refused: "symbol not found", "too few units"), as they are named after the change: what the
   *  reset options reset. */
  processed: string[];
  /** The message panel: one line per symbol, the failures marked `***`. */
  report: string[];
  changed: number;
}

/** `processSymbols` for `MODE::CHANGE`: the verbs that give every matched reference the new library symbol, and a report line for each. */
export function planChange(refs: readonly string[], symbols: readonly SymbolRow[], lib: LibInfo, opts: ChangeOptions): ChangePlan {
  const cmds: Cmd[] = [];
  const report: string[] = [];
  const processed: string[] = [];
  let changed = 0;
  const shown = opts.shown ?? ((r: string) => r);
  for (const ref of refs) {
    const units = symbols.filter((s) => s.id === ref);
    const was = units[0]?.lib_id ?? "";
    if (!lib.found) {
      report.push(`${shown(ref)}: ${opts.newLibId} *** symbol not found ***`);
      continue;
    }
    if (units.some((u) => u.unit > lib.unitCount)) {
      report.push(`${shown(ref)}: ${was} -> ${opts.newLibId} *** new symbol has too few units ***`);
      continue;
    }
    if (units.every((u) => u.lib_id === opts.newLibId) && !opts.updateReference && !opts.updateValue) {
      report.push(`${shown(ref)}: already ${opts.newLibId}`);
      processed.push(ref);
      continue;
    }
    const steps: Cmd[] = [];
    if (!units.every((u) => u.lib_id === opts.newLibId)) steps.push({ op: "sch_edit", verb: "change_symbol", id: ref, lib_id: opts.newLibId });
    // `resetText = libField->GetText().IsEmpty() ? m_resetEmptyFields : m_resetFieldText`.
    if (opts.updateValue && (lib.value !== "" || opts.resetEmpty) && units.some((u) => (u.value ?? "") !== lib.value)) steps.push({ op: "edit_symbol_fields", id: ref, value: lib.value });
    let newRef = ref;
    if (opts.updateReference && (lib.prefix !== "" || opts.resetEmpty)) newRef = reprefix(ref, lib.prefix);
    // Renaming last: the field edits above name the reference as it still is.
    if (newRef !== ref) steps.push({ op: "rename_symbol", id: ref, new_id: newRef });
    if (steps.length === 0) {
      report.push(`${shown(ref)}: nothing to change`);
      processed.push(ref);
      continue;
    }
    cmds.push(...steps);
    processed.push(newRef);
    changed++;
    report.push(`${newRef}: ${was} -> ${opts.newLibId}${newRef !== ref ? ` (was ${ref})` : ""}`);
  }
  return { cmds, report, processed, changed };
}

/** The reset options of the dialog (`m_resetFieldVisibilities`, `m_resetFieldEffects`, `m_resetFieldPositions`). */
export interface ResetOptions {
  visibility: boolean;
  effects: boolean;
  positions: boolean;
}

/** `reset_symbol_fields` on `refs` for the options ticked, or null when none is (or there is no symbol). `inStep`: sent with the change in its step, where a symbol
 *  already as the library has it is no refusal (`allow_unchanged`). */
export function resetCmd(refs: readonly string[], o: ResetOptions, inStep: boolean): Cmd | null {
  if (refs.length === 0 || (!o.visibility && !o.effects && !o.positions)) return null;
  return { op: "sch_edit", verb: "reset_symbol_fields", ids: [...refs], visibility: o.visibility, effects: o.effects, positions: o.positions, ...(inStep ? { allow_unchanged: true } : {}) } as unknown as Cmd;
}

/** One screen's part of the step: its commands (the change, then the reset of what it processed) addressed to the sheet path that shows it. */
export interface ScreenStep {
  sheet: string;
  cmds: Cmd[];
  processed: readonly string[];
}

/**
 * The one command of a Change / Update (`processSymbols`' one `SCH_COMMIT`, "Change Symbols" / "Update Symbols"): every screen's change commands and the resets
 * of the symbols it processed, each under `on_sheet`, in one batch from the root. With no change command at all, the resets alone (refused when they change
 * nothing). Null when there is nothing to send.
 */
export function changeStep(screens: readonly ScreenStep[], o: ResetOptions, lead: readonly Cmd[] = []): Cmd | null {
  const onSheet = (sheet: string, cmd: Cmd): Cmd => ({ op: "on_sheet", sheet, cmd }) as unknown as Cmd;
  const changes = lead.length + screens.reduce((n, s) => n + s.cmds.length, 0);
  const cmds: Cmd[] = [...lead];
  for (const s of screens) {
    cmds.push(...s.cmds.map((c) => onSheet(s.sheet, c)));
    const reset = resetCmd(s.processed, o, changes > 0);
    if (reset) cmds.push(onSheet(s.sheet, reset));
  }
  if (cmds.length === 0) return null;
  // one undo step from the root, so each `on_sheet` inside names its path from the root (an `on_sheet` inside another is relative to it)
  return onSheet("", { op: "batch", cmds } as unknown as Cmd);
}
