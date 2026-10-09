// The schematic selection filter -- `SCH_SELECTION_FILTER_OPTIONS` (include/project/sch_project_settings.h), what
// `PANEL_SCH_SELECTION_FILTER` (eeschema/widgets/panel_sch_selection_filter.cpp) edits and `SCH_SELECTION_TOOL::itemPassesFilter`
// (eeschema/tools/sch_selection_tool.cpp, 8303b2ad) applies to every click, box selection, Select All and cursor pick-up.
//
// Left out: KiCad's "Pins" (a pin cannot be selected on its own here) and "Images" (the studio has no bitmap items) -- a switch
// that gates nothing would only mislead. A locked item (kicad-port/schLock.ts) is selectable only with "Locked items" on.
import type { Schematic } from "../api/types";
import { fieldItems, parseFieldId } from "./schFieldEdit";

export interface SchSelectionFilter {
  /** Allow selecting locked items. */
  lockedItems: boolean;
  /** Symbols, power symbols and sheets. */
  symbols: boolean;
  /** Text, text boxes (and fields). */
  text: boolean;
  /** Wires, buses and junctions. */
  wires: boolean;
  /** Local, global and hierarchical labels. */
  labels: boolean;
  /** Graphic lines and shapes. */
  graphics: boolean;
  ruleAreas: boolean;
  /** Anything not fitting one of the above: no-connects, bus entries, directive labels. */
  otherItems: boolean;
}

export type SchFilterCategory = Exclude<keyof SchSelectionFilter, "lockedItems">;

/** The categories in the panel's order, with the labels `OnLanguageChanged` gives the checkboxes. */
export const SCH_FILTER_CATEGORIES: ReadonlyArray<{ key: SchFilterCategory; label: string }> = [
  { key: "symbols", label: "Symbols" },
  { key: "text", label: "Text" },
  { key: "wires", label: "Wires" },
  { key: "labels", label: "Labels" },
  { key: "graphics", label: "Graphics" },
  { key: "ruleAreas", label: "Rule Areas" },
  { key: "otherItems", label: "Other items" },
];

/** `SCH_SELECTION_FILTER_OPTIONS::SetDefaults` (and project_local_settings.cpp's JSON defaults): everything selectable but locked items. */
export const DEFAULT_SCH_SELECTION_FILTER: SchSelectionFilter = { lockedItems: false, symbols: true, text: true, wires: true, labels: true, graphics: true, ruleAreas: true, otherItems: true };

/** `SCH_SELECTION_FILTER_OPTIONS::All`: every category on ("locked items" is a switch of its own). */
export const allCategoriesOn = (f: SchSelectionFilter): boolean => SCH_FILTER_CATEGORIES.every((c) => f[c.key]);

/** `OnFilterChanged` for "All items": every category follows the checkbox, locked items stay as they were. */
export function setAllCategories(f: SchSelectionFilter, on: boolean): SchSelectionFilter {
  const next = { ...f };
  for (const c of SCH_FILTER_CATEGORIES) next[c.key] = on;
  return next;
}

/** `onPopupSelection` ("Only <category>" in a checkbox's right-click menu): that category on, the rest off. */
export function onlyCategory(f: SchSelectionFilter, only: SchFilterCategory): SchSelectionFilter {
  const next = setAllCategories(f, false);
  next[only] = true;
  return next;
}

/** The category `itemPassesFilter` files each of the sheet's items under, by id. */
export function categoryById(sch: Schematic): Map<string, SchFilterCategory> {
  const out = new Map<string, SchFilterCategory>();
  const put = (items: ReadonlyArray<{ id?: string }> | undefined, c: SchFilterCategory) => {
    for (const i of items ?? []) if (i.id) out.set(i.id, c);
  };
  put(sch.symbols, "symbols"); // SCH_SYMBOL_T (a power symbol is one too)
  put(sch.power_symbols, "symbols");
  put(sch.sheets, "symbols"); // SCH_SHEET_T
  put(sch.wires, "wires"); // SCH_LINE_T on LAYER_WIRE / LAYER_BUS
  put(sch.junctions, "wires"); // SCH_JUNCTION_T
  put(sch.lines, "graphics"); // SCH_LINE_T on the notes layer
  put(sch.labels, "labels"); // SCH_LABEL_T, SCH_GLOBAL_LABEL_T, SCH_HIER_LABEL_T
  put(sch.texts, "text"); // SCH_TEXT_T
  put(sch.no_connects, "otherItems"); // anything the switch has no case for
  put(sch.bus_entries, "otherItems");
  for (const g of sch.graphics ?? []) {
    if (!g.id) continue;
    // SCH_TEXTBOX_T is text, SCH_RULE_AREA_T has its own switch, SCH_DIRECTIVE_LABEL_T has no case (other items), the rest are SCH_SHAPE_T.
    out.set(g.id, g.shape.type === "text_box" ? "text" : g.shape.type === "rule_area" ? "ruleAreas" : g.shape.type === "directive" ? "otherItems" : "graphics");
  }
  // SCH_FIELD_T is text
  for (const f of fieldItems(sch)) out.set(f.id, "text");
  return out;
}

/** The id a field is locked by: the item that has it (`SCH_FIELD::IsLocked` is its parent's), a symbol by its reference whatever its unit. */
function lockKey(id: string): string {
  const f = parseFieldId(id);
  if (!f) return id;
  const unit = /^(.*)#(\d+)$/.exec(f.owner);
  return unit ? unit[1]! : f.owner;
}

/** `itemPassesFilter` for the sheet: may an item with this id be selected? An id the sheet does not know passes (it is not ours to filter). */
export function schSelectable(sch: Schematic, filter: SchSelectionFilter): (id: string) => boolean {
  const locked = new Set(sch.locked ?? []);
  const categories = categoryById(sch);
  return (id) => {
    if (locked.has(lockKey(id)) && !filter.lockedItems) return false;
    const c = categories.get(id);
    return c === undefined || filter[c];
  };
}
