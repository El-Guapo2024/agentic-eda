// The library trees' columns and their chooser, Select Columns... (`common.Control.selectColumns`, `ACTIONS::selectLibTreeColumns`): KiCad 8303b2ad's
// common/widgets/lib_tree.cpp `LIB_TREE::onHeaderContextMenu` (a right click on the tree's column header offers it), common/lib_tree_model_adapter.cpp
// (`loadColumnConfig`, `recreateColumns`, `addColumnIfNecessary`, `SetShownColumns`, `GetValue`), common/lib_tree_model.cpp (`RebuildSearchTerms`) and the
// dialog it opens, `EDA_REORDERABLE_LIST_DIALOG` (common/dialogs/eda_reorderable_list_dialog.cpp): "Available:" and "Enabled:" lists, `>` and `<`, Move up and
// Move down, OK and Cancel. Pure rules here; components/library/SelectColumnsDialog.tsx draws the dialog and state/libTreeColumns.ts keeps the choice.

/** `ACTIONS::selectLibTreeColumns` (`common.Control.selectColumns`, "Select Columns...", hidden from the toolbars): the one entry of the tree header's menu. */
export const SELECT_COLUMNS_ACTION = "common.Control.selectColumns";
export const SELECT_COLUMNS_LABEL = "Select Columns...";

/** The column that is always there, first (`recreateColumns`: "The Item column is always shown"). */
export const ITEM = "Item";
export const DESCRIPTION = "Description";

/** `loadColumnConfig` with nothing saved, and `m_availableColumns` before any library adds a field. */
export const DEFAULT_COLUMNS: readonly string[] = [ITEM, DESCRIPTION];

/** `loadColumnConfig`: the saved columns, or Item and Description when none are; Item put first when the saved list does not start with it. */
export function loadShownColumns(saved: readonly string[] | null | undefined): string[] {
  const shown = saved && saved.length > 0 ? saved.filter((c) => typeof c === "string" && c !== "") : [...DEFAULT_COLUMNS];
  if (shown.length === 0) return [...DEFAULT_COLUMNS];
  return shown[0] === ITEM ? shown : [ITEM, ...shown];
}

/** `recreateColumns` + `createMissingColumns`: the tree's columns in order -- Item, then each shown column once (a shown "Item" further down is the first one). */
export function treeColumns(shown: readonly string[]): string[] {
  const out = [ITEM];
  for (const c of shown) if (!out.includes(c)) out.push(c);
  return out;
}

/**
 * `m_availableColumns`: Item and Description, then each extra field the libraries have offered, in the order they came (`addColumnIfNecessary`, called for
 * every library loaded with `GetAvailableExtraFields`: a symbol library's chooser fields sorted by name, `SCH_IO_KICAD_SEXPR::GetAvailableSymbolFields`).
 * A footprint library has none (`FOOTPRINT` has no chooser fields), so the footprint trees offer Item and Description only.
 */
export function availableColumns(extraFieldsPerLibrary: Iterable<Iterable<string>>): string[] {
  const out = [...DEFAULT_COLUMNS];
  for (const fields of extraFieldsPerLibrary) {
    // `std::set<wxString>`: sorted, by code point
    const sorted = [...new Set(fields)].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
    for (const f of sorted) if (!out.includes(f)) out.push(f);
  }
  return out;
}

/** `GetAvailableSymbolFields` of one library's items: every chooser field name any of them has. */
export function extraFieldsOf(items: Iterable<{ fields?: Readonly<Record<string, string>> }>): string[] {
  const names = new Set<string>();
  for (const it of items) for (const k of Object.keys(it.fields ?? {})) names.add(k);
  return [...names];
}

/** `LIB_TREE_MODEL_ADAPTER::GetValue` for a column other than Item: Description is the description, any other the field of that name (empty when the item has none); line breaks become spaces. */
export function cellText(item: { description?: string; fields?: Readonly<Record<string, string>> }, column: string): string {
  const v = column === DESCRIPTION ? (item.description ?? "") : (item.fields?.[column] ?? "");
  return v.replace(/\n/g, " ");
}

/** `LIB_TREE_NODE::RebuildSearchTerms`: the values of the shown extra columns are search words of their own, weighted 4. */
export function shownColumnTerms(item: { fields?: Readonly<Record<string, string>> }, shown: readonly string[]): Array<{ text: string; score: number }> {
  return Object.entries(item.fields ?? {})
    .filter(([name]) => shown.includes(name))
    .map(([, text]) => ({ text, score: 4 }));
}

// ---------------------------------------------------------------------------------------------------------------------------------------- the dialog

/** `EDA_REORDERABLE_LIST_DIALOG`'s two lists and their selections (`m_selectedAvailable`, `m_selectedEnabled`; -1 for none). */
export interface ListDialog {
  available: string[];
  enabled: string[];
  selectedAvailable: number;
  selectedEnabled: number;
}

/** The dialog opened on every column the adapter offers and the ones shown (`updateItems`: an enabled column is not listed as available). */
export function openListDialog(all: readonly string[], enabled: readonly string[]): ListDialog {
  const on = new Set(enabled);
  return { available: all.filter((c) => !on.has(c)), enabled: [...enabled], selectedAvailable: 0, selectedEnabled: 0 };
}

/** `onAddItem` (`>`): the selected available column goes after the selected enabled one (the end when none is). Nothing selected: nothing (KiCad rings the bell). */
export function addColumn(d: ListDialog): ListDialog {
  const item = d.available[d.selectedAvailable];
  if (item === undefined) return d;
  const available = d.available.filter((_, i) => i !== d.selectedAvailable);
  const pos = Math.min(d.selectedEnabled + 1, d.enabled.length);
  const enabled = [...d.enabled.slice(0, pos), item, ...d.enabled.slice(pos)];
  return { available, enabled, selectedAvailable: Math.min(d.selectedAvailable, available.length - 1), selectedEnabled: d.selectedEnabled };
}

/** `onRemoveItem` (`<`): the selected enabled column goes back to the available list (at the selected available place); the first one (Item) cannot be removed. */
export function removeColumn(d: ListDialog): ListDialog {
  const item = d.enabled[d.selectedEnabled];
  if (item === undefined || d.selectedEnabled === 0) return d;
  const enabled = d.enabled.filter((_, i) => i !== d.selectedEnabled);
  const pos = Math.max(0, d.selectedAvailable);
  const available = [...d.available.slice(0, pos), item, ...d.available.slice(pos)];
  return { available, enabled, selectedAvailable: d.selectedAvailable, selectedEnabled: Math.min(d.selectedEnabled, enabled.length - 1) };
}

/** `onMoveUp`: the selected enabled column swaps with the one above it (not the first). */
export function moveColumnUp(d: ListDialog): ListDialog {
  const i = d.selectedEnabled;
  if (i <= 0 || i >= d.enabled.length) return d;
  const enabled = [...d.enabled];
  [enabled[i - 1], enabled[i]] = [enabled[i]!, enabled[i - 1]!];
  return { ...d, enabled, selectedEnabled: i - 1 };
}

/** `onMoveDown`: the selected enabled column swaps with the one below it (not the last). */
export function moveColumnDown(d: ListDialog): ListDialog {
  const i = d.selectedEnabled;
  if (i < 0 || i >= d.enabled.length - 1) return d;
  const enabled = [...d.enabled];
  [enabled[i + 1], enabled[i]] = [enabled[i]!, enabled[i + 1]!];
  return { ...d, enabled, selectedEnabled: i + 1 };
}
