// Bulk Edit Symbol Library Links (`eeschema.EditorControl.editSymbolLibraryLinks`, DIALOG_EDIT_SYMBOLS_LIBID, eeschema/dialogs/dialog_edit_symbols_libid.cpp at
// 8303b2ad): the symbols of the schematic grouped by the library symbol they use, and the new link each group takes.

export interface LibLinkRow {
  /** The library id the group uses now. */
  libId: string;
  /** The references in the group, in order, one entry per reference (units of one part count once). */
  refs: string[];
  /** The group's symbol cannot be found in any library (`unitcount == 0`). */
  orphan: boolean;
}

const refCompare = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/**
 * `initDlg`: every symbol sorted by library id and then reference, grouped by library id; a group is an orphan when its symbol is in no library
 * (`known` is every library id that exists). A reference that repeats within a group (the units of a multi-unit part) is listed once.
 */
export function libLinkRows(symbols: ReadonlyArray<{ id: string; lib_id: string | null }>, known: ReadonlySet<string>): LibLinkRow[] {
  const sorted = symbols.map((s) => ({ id: s.id, libId: s.lib_id ?? "" })).sort((a, b) => refCompare(a.libId, b.libId) || refCompare(a.id, b.id));
  const rows: LibLinkRow[] = [];
  for (const s of sorted) {
    const last = rows[rows.length - 1];
    if (last && last.libId === s.libId) {
      if (!last.refs.includes(s.id)) last.refs.push(s.id);
    } else {
      rows.push({ libId: s.libId, refs: [s.id], orphan: !known.has(s.libId) });
    }
  }
  return rows;
}

/** `LIB_ID::IsValid`: a nickname and an item name. */
export function libIdIsValid(libId: string): boolean {
  const at = libId.indexOf(":");
  return at > 0 && at < libId.length - 1 && !libId.slice(at + 1).includes(":");
}

/** A library nickname the Export Symbols dialog may name a new library: letters, digits and `_ - . + `, with a space only inside (no colon, which separates it from the item, and no path characters). */
export function isLibraryNickname(name: string): boolean {
  return /^[A-Za-z0-9_.+\- ]+$/.test(name) && name.trim() === name;
}

/**
 * `onClickOrphansButton` ("Map Orphans"): for each orphan row, the known library symbols with the same item name -- the first one fills the cell, the
 * rest are the candidates to choose from. Returns, per orphan libId, the candidate list (empty when none was found).
 */
export function orphanCandidates(rows: readonly LibLinkRow[], allIds: readonly string[]): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const r of rows) {
    if (!r.orphan) continue;
    const item = r.libId.includes(":") ? r.libId.slice(r.libId.indexOf(":") + 1) : r.libId;
    out.set(r.libId, allIds.filter((id) => id.includes(":") && id.slice(id.indexOf(":") + 1) === item && id !== r.libId));
  }
  return out;
}

/** The changes the dialog commits (`TransferDataFromWindow`): rows with a new id that is not blank and not the current one. */
export function libLinkChanges(rows: readonly LibLinkRow[], edits: Readonly<Record<string, string>>): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const r of rows) {
    const next = (edits[r.libId] ?? "").trim();
    if (next !== "" && next !== r.libId) out.push([r.libId, next]);
  }
  return out;
}
