// Change Symbols / Update Symbols -- `DIALOG_CHANGE_SYMBOLS` (eeschema/dialogs/dialog_change_symbols.cpp at 8303b2ad): which symbols a request
// matches (`isMatch`), which of them can take the new library symbol (`processSymbols`' checks), and what the change does to their reference and value.
//
// KiCad's dialog also resets field visibilities, sizes, positions and attributes; the studio keeps none of those per symbol, so only the field
// text options (Reference, Value) and the library link are ported.
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
}

export interface ChangePlan {
  cmds: Cmd[];
  /** The message panel: one line per symbol, the failures marked `***`. */
  report: string[];
  changed: number;
}

/** `processSymbols` for `MODE::CHANGE`: the verbs that give every matched reference the new library symbol, and a report line for each. */
export function planChange(refs: readonly string[], symbols: readonly SymbolRow[], lib: LibInfo, opts: ChangeOptions): ChangePlan {
  const cmds: Cmd[] = [];
  const report: string[] = [];
  let changed = 0;
  for (const ref of refs) {
    const units = symbols.filter((s) => s.id === ref);
    const was = units[0]?.lib_id ?? "";
    if (!lib.found) {
      report.push(`${ref}: ${opts.newLibId} *** symbol not found ***`);
      continue;
    }
    if (units.some((u) => u.unit > lib.unitCount)) {
      report.push(`${ref}: ${was} -> ${opts.newLibId} *** new symbol has too few units ***`);
      continue;
    }
    if (units.every((u) => u.lib_id === opts.newLibId) && !opts.updateReference && !opts.updateValue) {
      report.push(`${ref}: already ${opts.newLibId}`);
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
      report.push(`${ref}: nothing to change`);
      continue;
    }
    cmds.push(...steps);
    changed++;
    report.push(`${newRef}: ${was} -> ${opts.newLibId}${newRef !== ref ? ` (was ${ref})` : ""}`);
  }
  return { cmds, report, changed };
}
