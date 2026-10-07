// Library item naming -- the pure rules behind the library-tree actions of the two library editors
// (`pcbnew.ModuleEditor.*` and `eeschema.SymbolLibraryControl.*`): which names are legal, how a
// duplicate or a paste picks a free name, and how the flat project library is grouped into the
// "library > item" tree the actions work on.
//
// The footprint/symbol libraries here are one flat list keyed by name / `lib_id` (`design.json`'s
// `footprint_library` / `symbol_library`), so a "library" is the nickname before the first colon of
// `Lib:Name` (a bare name belongs to the project's own library). The legality rules are the ones the
// backend (`crates/ops/src/library_editors.rs`) enforces, ported from `common/lib_id.cpp`
// (`LIB_ID::isLegalChar`, `isLegalLibraryNameChar`) and `pcbnew/footprint.cpp`
// (`FOOTPRINT::StringLibNameInvalidChars`).

/** The library a bare footprint name (or a symbol with no nickname) is shown under. */
export const PROJECT_LIBRARY = "eda";

/** `Lib:Name` -> `{ lib: "Lib", item: "Name" }`; a bare name has an empty `lib`. Only the first colon separates. */
export function splitLibName(name: string): { lib: string; item: string } {
  const i = name.indexOf(":");
  return i < 0 ? { lib: "", item: name } : { lib: name.slice(0, i), item: name.slice(i + 1) };
}

export function joinLibName(lib: string, item: string): string {
  return lib ? `${lib}:${item}` : item;
}

/** `LIB_ID::isLegalChar` with `illegal_filename_chars_allowed = false`: the first character a library item (symbol) name may not hold. */
export function libItemNameIllegalChar(name: string): string | null {
  for (const c of name) if (c === ":" || c === "\t" || c === "\n" || c === "\r" || c === "\\" || c === "<" || c === ">" || c === '"') return c;
  return null;
}

/** `FOOTPRINT::IsLibNameValid`'s set (`"%$<>\t\n\r\"\\/:"`): the first character a footprint name may not hold. */
export function footprintItemNameIllegalChar(name: string): string | null {
  for (const c of name) if ("%$<>\t\n\r\"\\/:".includes(c)) return c;
  return null;
}

/** `LIB_ID::isLegalLibraryNameChar`: no control character, backslash or colon in a library nickname. */
export function libraryNicknameIllegalChar(nick: string): string | null {
  for (const c of nick) if (c.charCodeAt(0) < 0x20 || c === "\\" || c === ":") return c;
  return null;
}

function show(c: string): string {
  return c === "\t" ? "a tab" : c === "\n" || c === "\r" ? "a line break" : `"${c}"`;
}

/** The message the backend would refuse `name` with, or `null` when it is a legal footprint name. */
export function footprintNameError(name: string): string | null {
  const { lib, item } = splitLibName(name);
  if (item.trim() === "") return "Footprint must have a name.";
  const l = libraryNicknameIllegalChar(lib);
  if (l) return `A library nickname cannot contain ${show(l)}.`;
  const c = footprintItemNameIllegalChar(item);
  return c ? `A footprint name cannot contain ${show(c)}.` : null;
}

/** The message the backend would refuse `libId` with, or `null` when it is a legal symbol `lib_id`. */
export function symbolLibIdError(libId: string): string | null {
  const { lib, item } = splitLibName(libId);
  if (item.trim() === "") return "Symbol must have a name.";
  const l = libraryNicknameIllegalChar(lib);
  if (l) return `A library nickname cannot contain ${show(l)}.`;
  const c = libItemNameIllegalChar(item);
  return c ? `A symbol name cannot contain ${show(c)}.` : null;
}

/** `EscapeString( name, CTX_LIBID )` (common/string_utils.cpp): the characters a library item name may not hold become `{backslash}`, `{lt}`, `{gt}`, `{colon}` and `{dblquote}`; line breaks are dropped. */
export function escapeLibIdName(name: string): string {
  let out = "";
  for (const c of name) {
    if (c === "\\") out += "{backslash}";
    else if (c === "<") out += "{lt}";
    else if (c === ">") out += "{gt}";
    else if (c === ":") out += "{colon}";
    else if (c === '"') out += "{dblquote}";
    else if (c === "\n" || c === "\r") continue;
    else out += c;
  }
  return out;
}

/** `SAVE_SYMBOL_AS_DIALOG::getSymbolName`: the typed name is trimmed, its spaces become underscores, and it is escaped for a `LIB_ID`. */
export function saveAsSymbolName(typed: string): string {
  return escapeLibIdName(typed.trim().replace(/ /g, "_"));
}

/**
 * `SYMBOL_EDIT_FRAME::ensureUniqueName` and `FOOTPRINT_EDIT_FRAME::DuplicateFootprint`: keep `name` when it is free,
 * else append `_1`, `_2`, ... to the original name until the result is unused
 * (`newName.Printf( "%s_%d", aSymbol->GetName(), i++ )`, `i` from 1).
 */
export function ensureUniqueName(name: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  if (!used.has(name)) return name;
  for (let i = 1; ; i++) {
    const candidate = `${name}_${i}`;
    if (!used.has(candidate)) return candidate;
  }
}

/** `ensureUniqueName` on the item part of a `Lib:Name` id, tested against whole ids (the library stays as given). */
export function ensureUniqueLibId(libId: string, takenLibIds: Iterable<string>): string {
  const used = new Set(takenLibIds);
  const { lib, item } = splitLibName(libId);
  const unique = ensureUniqueName(item, [...used].filter((id) => splitLibName(id).lib === lib).map((id) => splitLibName(id).item));
  return joinLibName(lib, unique);
}

/** `FOOTPRINT_EDITOR_CONTROL::PasteFootprint`: `while( FootprintExists( newLib, newName ) ) newName += "_copy";` -- the suffix piles up. */
export function pastedFootprintName(name: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  let n = name;
  while (used.has(n)) n += "_copy";
  return n;
}

export interface LibraryGroup<T extends { name: string }> {
  lib: string;
  items: T[];
}

/**
 * The tree the library panels show: the flat name list grouped by nickname (a bare name goes under `defaultLib`), groups and
 * items sorted by name, case-insensitively, the way the library tree sorts them.
 */
export function libraryGroups<T extends { name: string }>(items: readonly T[], defaultLib = PROJECT_LIBRARY): LibraryGroup<T>[] {
  const by = new Map<string, T[]>();
  for (const it of items) {
    const lib = splitLibName(it.name).lib || defaultLib;
    const list = by.get(lib) ?? [];
    list.push(it);
    by.set(lib, list);
  }
  const cmp = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });
  return [...by.entries()]
    .sort(([a], [b]) => cmp(a, b))
    .map(([lib, list]) => ({ lib, items: [...list].sort((a, b) => cmp(splitLibName(a.name).item, splitLibName(b.name).item)) }));
}
