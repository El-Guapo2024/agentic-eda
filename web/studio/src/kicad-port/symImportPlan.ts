// Symbols brought into the project library from a file or the clipboard, with the derived ones among them kept derived: what `SYMBOL_EDIT_FRAME::ImportSymbol` and
// `DuplicateSymbol( true )` (Paste) hand to `LIB_SYMBOL_LIBRARY_MANAGER::UpdateSymbol`. The reader gives each symbol's `extends` as the parent's bare name (the file's, or
// the clipboard's, own naming); the library takes a derived symbol only after its parent, so a plan lists parents first, brings along a parent the person did not tick
// (`DIALOG_IMPORT_SYMBOL_SELECT` lists the file's symbols one by one) and follows a parent that is given another name to stay unique.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibrarySymbol } from "../api/types";
import { bareName } from "./symFields";

/** A derived symbol's parent in the list (by bare name), when the list has it. */
const parentIn = (list: readonly LibrarySymbol[], s: LibrarySymbol): LibrarySymbol | undefined => (s.extends ? list.find((p) => bareName(p.lib_id) === bareName(s.extends!)) : undefined);

/** The list reordered so that no symbol comes before the symbol it extends (a stable pass: the order is the list's otherwise; a cycle ends where it closes). */
export function parentsFirst(list: readonly LibrarySymbol[]): LibrarySymbol[] {
  const out: LibrarySymbol[] = [];
  const state = new Map<LibrarySymbol, "visiting" | "done">();
  const visit = (s: LibrarySymbol) => {
    if (state.has(s)) return;
    state.set(s, "visiting");
    const p = parentIn(list, s);
    if (p) visit(p);
    state.set(s, "done");
    out.push(s);
  };
  for (const s of list) visit(s);
  return out;
}

/**
 * What an import writes: the `chosen` symbols and, for each derived one whose parent is neither chosen nor already in the library (`present`: bare names of the library the
 * symbols land in), that parent from `file` and its own parents -- parents first. A parent the library already has stays the link (the child follows it by name).
 */
export function planImport(file: readonly LibrarySymbol[], chosen: readonly LibrarySymbol[], present: ReadonlySet<string>): LibrarySymbol[] {
  const wanted = new Set<LibrarySymbol>(chosen);
  const pull = (s: LibrarySymbol, depth: number) => {
    if (depth > 64 || !s.extends || present.has(bareName(s.extends))) return;
    const p = parentIn(file, s);
    if (!p || wanted.has(p)) return;
    wanted.add(p);
    pull(p, depth + 1);
  };
  for (const s of [...chosen]) pull(s, 0);
  // in the file's order, then parents first
  return parentsFirst(file.filter((s) => wanted.has(s)).concat(chosen.filter((s) => !file.includes(s))));
}

/**
 * Paste's names: every symbol of the clipboard gets a free name in `lib` (`ensureUniqueName`, so `R` becomes `R_1` when taken), and a derived symbol follows the new name of
 * its parent when the parent was pasted with it. `unique(libId, taken)` is `ensureUniqueLibId`; `join` is `joinLibName`. `puts` are the symbols to put, parents first;
 * `ids[i]` is the new `lib_id` of `clipboard[i]` (Paste selects the first one).
 */
export function planPaste(
  clipboard: readonly LibrarySymbol[],
  lib: string,
  taken: readonly string[],
  unique: (libId: string, taken: readonly string[]) => string,
  join: (lib: string, item: string) => string
): { puts: LibrarySymbol[]; ids: string[] } {
  const used = [...taken];
  const renamed = new Map<string, string>(); // old bare name -> new bare name
  const newId = new Map<LibrarySymbol, string>();
  const puts: LibrarySymbol[] = [];
  for (const s of parentsFirst(clipboard)) {
    const old = bareName(s.lib_id);
    const libId = unique(join(lib, old), used);
    used.push(libId);
    renamed.set(old, bareName(libId));
    newId.set(s, libId);
    const parent = s.extends ? renamed.get(bareName(s.extends)) ?? bareName(s.extends) : null;
    puts.push({ ...s, lib_id: libId, extends: parent });
  }
  return { puts, ids: clipboard.map((s) => newId.get(s)!) };
}
