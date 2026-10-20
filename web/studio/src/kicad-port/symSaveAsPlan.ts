// Save Symbol As / Save Copy As for a derived symbol (`SYMBOL_EDIT_FRAME::saveSymbolCopyAs`, `SYMBOL_SAVE_AS_HANDLER::DoSave`, `CheckSavingIntoOwnInheritance`,
// `CheckForParentalChainConflicts`, eeschema/symbol_editor/symbol_editor.cpp). A derived symbol saved under a name of its own library stays derived from the same parent;
// saved into another library it takes its parents along (root first, each linked to the copy of its parent), unless "Flatten/remove symbol inheritance" is ticked, which
// saves the one flat symbol. A name the target library already has is overwritten, or -- when the parents conflict too and the person chose "Rename All" -- every copy gets
// the first free `name_N`.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibrarySymbol } from "../api/types";
import { bareName, libraryOf } from "./symFields";

export type ConflictStrategy = "overwrite" | "rename";

/** Everything the plan needs of the symbol being saved. */
export interface SaveAsInput {
  /** The symbol and its parents, the symbol first and the root last: their own entries (`GetParentChain( symbol, true )`). */
  chain: readonly LibrarySymbol[];
  /** The symbol with everything it inherits (`Flatten()`); used when `flatten`. */
  flat: LibrarySymbol;
  newLibId: string;
  flatten: boolean;
  strategy: ConflictStrategy;
  /** The `lib_id`s that exist (the project library's and the ones the editor lists). */
  taken: ReadonlySet<string>;
}

const sameLibrary = (a: string, b: string) => libraryOf(a) === libraryOf(b);

/** `CheckSavingIntoOwnInheritance`: the name is in the symbol's ancestry, or the symbol is in the ancestry of the one that would be replaced. Only inside one library, and only for a name in use. */
export function savingIntoOwnInheritance(chain: readonly LibrarySymbol[], newLibId: string, taken: ReadonlySet<string>, parentOf: (libId: string) => string | undefined): { inAncestry: boolean; inDescendants: boolean } {
  const leaf = chain[0]!;
  if (!sameLibrary(leaf.lib_id, newLibId) || !taken.has(newLibId)) return { inAncestry: false, inDescendants: false };
  const inAncestry = chain.slice(1).some((p) => bareName(p.lib_id) === bareName(newLibId));
  let inDescendants = false;
  for (let at = parentOf(newLibId), n = 0; at && n < 64; at = parentOf(at), n++) {
    if (bareName(at) === bareName(leaf.lib_id)) {
      inDescendants = true;
      break;
    }
  }
  return { inAncestry, inDescendants };
}

/** `CheckForParentalChainConflicts`: the names (bare) the save would put on top of existing symbols. */
export function chainConflicts(input: Pick<SaveAsInput, "chain" | "newLibId" | "flatten" | "taken">): string[] {
  const { chain, newLibId, flatten, taken } = input;
  const lib = libraryOf(newLibId);
  const inUse = (bare: string) => taken.has(lib ? `${lib}:${bare}` : bare);
  const out: string[] = [];
  if (sameLibrary(chain[0]!.lib_id, newLibId) || flatten) {
    if (inUse(bareName(newLibId))) out.push(bareName(newLibId));
    return out;
  }
  chain.forEach((s, i) => {
    const bare = i === 0 ? bareName(newLibId) : bareName(s.lib_id);
    if (inUse(bare)) out.push(bare);
  });
  return out;
}

/** The `Value` of a symbol that names itself: stored text equal to the old name follows the new one (`valueFollowsName`). */
function followName(sym: LibrarySymbol, newBare: string): LibrarySymbol {
  const fields = (sym.fields ?? []).map((f) => (f.name === "Value" ? { ...f, text: newBare } : f));
  return { ...sym, fields };
}

/** `SYMBOL_SAVE_AS_HANDLER::DoSave`: the symbols to put, root first. */
export function savePlan(input: SaveAsInput): LibrarySymbol[] {
  const { chain, flat, newLibId, flatten, strategy, taken } = input;
  const lib = libraryOf(newLibId);
  const leaf = chain[0]!;
  const valueFollowsName = (leaf.fields ?? []).find((f) => f.name === "Value")?.text === bareName(leaf.lib_id);
  const sameLib = sameLibrary(leaf.lib_id, newLibId);
  const used = new Set(taken);
  const resolve = (proposed: string): string => {
    if (strategy === "overwrite") return proposed;
    for (let n = 1; ; n++) {
      const candidate = `${proposed}_${n}`;
      if (!used.has(lib ? `${lib}:${candidate}` : candidate)) return candidate;
    }
  };
  // what is copied, leaf first: the flat symbol; the symbol itself into its own library (it keeps its parent); the whole chain into another one
  const source: LibrarySymbol[] = flatten ? [{ ...flat, extends: null }] : sameLib ? [leaf] : [...chain];
  const out: LibrarySymbol[] = [];
  let newParent: string | null = null;
  for (let i = source.length - 1; i >= 0; i--) {
    const old = source[i]!;
    const bare = resolve(i === 0 ? bareName(newLibId) : bareName(old.lib_id));
    const libId = lib ? `${lib}:${bare}` : bare;
    let next: LibrarySymbol = { ...old, lib_id: libId, published: false };
    if (valueFollowsName) next = followName(next, bare);
    if (i !== source.length - 1) next = { ...next, extends: newParent }; // the copy of its parent, in the new library
    used.add(libId);
    newParent = libId;
    out.push(next);
  }
  return out;
}
