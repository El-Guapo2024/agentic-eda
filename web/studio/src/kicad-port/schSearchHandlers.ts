// The schematic editor's Search pane (`SCH_SEARCH_PANE`, eeschema/widgets/sch_search_pane.cpp, and its handlers, eeschema/widgets/search_handlers.cpp at
// 8303b2ad, over common/widgets/search_pane.cpp and search_pane_tab.cpp; UI-LEFT-2 SS10): the five tabs, what each finds for a query, its columns and cells.
//
//   * Symbols (`SYMBOL_SEARCH_HANDLER`): every placed symbol that is not a power symbol whose fields match (`SCH_FIELD::Matches`: a hidden field only with
//     "Search Hidden Fields"; the Reference also when its symbol matches -- with "Search Metadata" its library name, description or keywords -- or its
//     reference on the sheet path does, with the unit after it for a symbol of several units). Reference, Value, Footprint, Page, X, Y, Excl. Sim, Excl. BOM,
//     Excl. Board, DNP, Library Link, Library Description.
//   * Power (`POWER_SEARCH_HANDLER`): the power symbols the same way. Reference, Value, Page, X, Y.
//   * Text (`TEXT_SEARCH_HANDLER`): the texts and text boxes by their text as stored (`SCH_TEXT::Matches( GetText() )`). Type ("Text", "Text Box"), Text (the
//     shown text), Page, X, Y.
//   * Labels (`LABEL_SEARCH_HANDLER`): the local, global, hierarchical and directive labels by their text unescaped (`SCH_LABEL_BASE::Matches`). Type
//     ("Local", "Global", "Hierarchical", "Directive"), Name, Page, X, Y.
//   * Groups (`GROUP_SEARCH_HANDLER`): by name unescaped (`SCH_GROUP::Matches`). Name, Page, X, Y (the centre of its box, `SCH_GROUP::GetPosition`).
//
// `SCH_SEARCH_HANDLER::FindAll` walks every sheet path of the hierarchy (each screen once per path that shows it), the items of its screen; an empty query
// lists everything. Matching is `EDA_ITEM::Matches` in `EDA_SEARCH_MATCH_MODE::PERMISSIVE` (kicad-port/edaPatternMatch.ts). After each search the rows are
// sorted by the tab's sort column with `StrNumCmp( .., true )` (`SCH_SEARCH_HANDLER::Sort`). X and Y are `MessageTextFromValue` of the position.
//
// **Differs**: a symbol's library graphic texts do not make its Reference match (`SCH_SYMBOL::Matches`' "non-field lib draw items"); a group's box is the
// union of its members' boxes as the canvas draws them (kicad-port/schGroups.ts).
//
// Pure: no store, no DOM. Unit-tested in schSearchHandlers.test.ts.
import type { LibSymbol, Schematic, SchematicSymbol } from "../api/types";
import { permissiveMatches } from "./edaPatternMatch";
import { messageTextFromValue, type MessageUnits } from "./messageText";
import { unescapeString } from "./schNetName";
import { strNumCmp } from "./schSheetPins";
import { referenceUnitSuffix, subpartIdsOf } from "./schUnits";
import type { SearchColumn, SearchPaneSettings } from "./searchHandlers";

export type SchSearchTabId = "symbols" | "power" | "text" | "labels" | "groups";

const X: SearchColumn = { name: "X", proportion: 3, align: "center" };
const Y: SearchColumn = { name: "Y", proportion: 3, align: "center" };

/** The tabs in the order `SCH_SEARCH_PANE` adds them, each with its name and columns (`m_columns.emplace_back( _HKI( "Reference" ), 2, wxLIST_FORMAT_LEFT )` ...). */
export const SCH_SEARCH_TABS: readonly { id: SchSearchTabId; name: string; columns: readonly SearchColumn[] }[] = [
  {
    id: "symbols",
    name: "Symbols",
    columns: [
      { name: "Reference", proportion: 2, align: "left" },
      { name: "Value", proportion: 6, align: "left" },
      { name: "Footprint", proportion: 10, align: "left" },
      { name: "Page", proportion: 1, align: "center" },
      X,
      Y,
      { name: "Excl. Sim", proportion: 2, align: "center" },
      { name: "Excl. BOM", proportion: 2, align: "center" },
      { name: "Excl. Board", proportion: 2, align: "center" },
      { name: "DNP", proportion: 2, align: "center" },
      { name: "Library Link", proportion: 8, align: "left" },
      { name: "Library Description", proportion: 10, align: "left" },
    ],
  },
  {
    id: "power",
    name: "Power",
    columns: [{ name: "Reference", proportion: 2, align: "left" }, { name: "Value", proportion: 6, align: "left" }, { name: "Page", proportion: 1, align: "center" }, X, Y],
  },
  {
    id: "text",
    name: "Text",
    columns: [{ name: "Type", proportion: 2, align: "left" }, { name: "Text", proportion: 12, align: "left" }, { name: "Page", proportion: 1, align: "center" }, X, Y],
  },
  {
    id: "labels",
    name: "Labels",
    columns: [{ name: "Type", proportion: 2, align: "left" }, { name: "Name", proportion: 6, align: "left" }, { name: "Page", proportion: 2, align: "center" }, X, Y],
  },
  { id: "groups", name: "Groups", columns: [{ name: "Name", proportion: 6, align: "left" }, { name: "Page", proportion: 2, align: "center" }, X, Y] },
];

/** One sheet path of the hierarchy as `FindAll` walks it: the placement ids from the root, its page (`GetPageNumber`) and the schematic its screen shows there. */
export interface SchSearchSheet {
  path: string[];
  page: string;
  sch: Schematic;
}

/** What the handlers read beside the sheets: the user's units, the pane's settings, and a group's centre on a sheet (`SCH_GROUP::GetPosition`). */
export interface SchSearchContext {
  units: MessageUnits;
  settings: SearchPaneSettings;
  groupCentre?: (sheet: SchSearchSheet, groupId: string) => readonly [number, number] | null;
}

/** One row: `id` is unique in the tab (the sheet path and the item), `sheet` the path it is on, `item` the id a selection of it selects, `at` its position. */
export interface SchSearchRow {
  id: string;
  sheet: string[];
  item: string;
  at: readonly [number, number];
  cells: string[];
}

/** A field of an item as `SCH_FIELD::Matches` reads it: its name, its text, whether it is shown. */
interface FieldText {
  name: string;
  text: string;
  visible: boolean;
}

const libOf = (sch: Schematic, libId: string | null): LibSymbol | undefined => (libId ? sch.lib_symbols?.[libId] : undefined);

/** A symbol's fields in KiCad's order (the ones the sheet sends, else Reference and Value shown, Footprint, Datasheet and Description hidden). */
function symbolFields(sym: SchematicSymbol, lib: LibSymbol | undefined): FieldText[] {
  const own = (sym.fields ?? []).map((f) => ({ name: f.name, text: f.text, visible: f.visible }));
  const has = (n: string) => own.some((f) => f.name === n);
  const out: FieldText[] = [...own];
  if (!has("Reference")) out.push({ name: "Reference", text: sym.reference ?? sym.id, visible: true });
  if (!has("Value")) out.push({ name: "Value", text: sym.value ?? "", visible: true });
  if (!has("Footprint")) out.push({ name: "Footprint", text: sym.footprint ?? "", visible: false });
  if (!has("Datasheet")) out.push({ name: "Datasheet", text: sym.datasheet ?? "", visible: false });
  // the fifth mandatory field (`FIELD_T::DESCRIPTION`): the library symbol's description, hidden
  if (!has("Description")) out.push({ name: "Description", text: lib?.description ?? "", visible: false });
  return out;
}

/** `EDA_ITEM::Matches( aText, frp )` with `PERMISSIVE`: an empty query matches everything (the handlers' `frp.findString.IsEmpty() ||`). */
const matches = (text: string, query: string) => permissiveMatches(text, query);

/**
 * `SYMBOL_SEARCH_HANDLER` / `POWER_SEARCH_HANDLER`'s test: any field `SCH_FIELD::Matches`. A hidden field only with "Search Hidden Fields"; the Reference when
 * its symbol matches (`SCH_SYMBOL::Matches`: the other fields, or with "Search Metadata" the library name, description or keywords) or its reference on this
 * path, or that reference with the unit after it.
 */
function fieldsMatch(fields: readonly FieldText[], ref: string, refWithUnit: string, meta: readonly string[], query: string, s: SearchPaneSettings): boolean {
  if (query === "") return true;
  const shown = (f: FieldText) => f.visible || s.searchHiddenFields;
  const others = fields.filter((f) => f.name !== "Reference");
  const symbolMatches = (s.searchMetadata && meta.some((m) => matches(m, query))) || others.some((f) => shown(f) && matches(unescapeString(f.text), query));
  if (symbolMatches) return true;
  const reference = fields.find((f) => f.name === "Reference");
  return !!reference && shown(reference) && (matches(ref, query) || (refWithUnit !== ref && matches(refWithUnit, query)));
}

const flag = (on: boolean | undefined) => (on ? "X" : " ");

/** `SCH_SEARCH_HANDLER::FindAll( collector )` for `tab`: the rows of every sheet path, in the order the walk finds them. */
export function schSearchTab(tab: SchSearchTabId, sheets: readonly SchSearchSheet[], query: string, c: SchSearchContext): SchSearchRow[] {
  const rows: SchSearchRow[] = [];
  const xy = (at: readonly [number, number]) => [messageTextFromValue(at[0], c.units), messageTextFromValue(at[1], c.units)];
  for (const sheet of sheets) {
    const { sch, page, path } = sheet;
    const row = (item: string, at: readonly [number, number], cells: string[]) => rows.push({ id: `${path.join("/")}#${item}`, sheet: path, item, at, cells });
    switch (tab) {
      case "symbols":
        for (const sym of sch.symbols) {
          const lib = libOf(sch, sym.lib_id);
          const ref = sym.reference ?? sym.id;
          // `GetRef( sheet, true )`: the unit's sub-reference after it for a symbol of several units
          const withUnit = ref + referenceUnitSuffix(lib, sym.path_unit ?? sym.unit, subpartIdsOf(sch));
          const meta = [sym.lib_id ?? "", lib?.description ?? "", lib?.keywords ?? ""];
          if (!fieldsMatch(symbolFields(sym, lib), ref, withUnit, meta, query, c.settings)) continue;
          const shownField = (name: string, fallback: string | null) => {
            const f = sym.fields?.find((x) => x.name === name);
            return f ? (f.shown ?? f.text) : (name === "Value" ? sym.shown?.value : sym.shown?.footprint) ?? fallback ?? "";
          };
          row(sym.id, sym.at, [
            withUnit,
            shownField("Value", sym.value),
            shownField("Footprint", sym.footprint),
            page,
            ...xy(sym.at),
            flag(sym.exclude_from_sim),
            flag(sym.exclude_from_bom),
            flag(sym.exclude_from_board),
            flag(sym.dnp),
            sym.lib_id ?? "",
            lib?.description ?? "",
          ]);
        }
        break;
      case "power":
        for (const ps of sch.power_symbols) {
          const ref = ps.reference ?? ps.id;
          const fields: FieldText[] = (ps.fields ?? []).map((f) => ({ name: f.name, text: f.text, visible: f.visible }));
          if (!fields.some((f) => f.name === "Reference")) fields.push({ name: "Reference", text: ref, visible: false });
          if (!fields.some((f) => f.name === "Value")) fields.push({ name: "Value", text: ps.net, visible: true });
          for (const f of ps.props?.fields ?? []) if (!fields.some((x) => x.name === f.name)) fields.push({ name: f.name, text: f.text, visible: f.visible ?? false });
          const lib = libOf(sch, ps.lib_id);
          if (!fieldsMatch(fields, ref, ref, [ps.lib_id, lib?.description ?? "", lib?.keywords ?? ""], query, c.settings)) continue;
          const value = ps.fields?.find((f) => f.name === "Value");
          row(ps.id, ps.at, [ref, value ? (value.shown ?? value.text) : unescapeString(ps.net), page, ...xy(ps.at)]);
        }
        break;
      case "text":
        for (const t of sch.texts) {
          if (query !== "" && !matches(t.content, query)) continue;
          row(t.id, t.at, ["Text", t.shown ?? t.content, page, ...xy(t.at)]);
        }
        for (const g of sch.graphics ?? []) {
          if (g.shape.type !== "text_box") continue;
          if (query !== "" && !matches(g.shape.text, query)) continue;
          // `SCH_TEXTBOX::GetPosition`: its start corner
          const at: [number, number] = [g.shape.start.x, g.shape.start.y];
          row(g.id, at, ["Text Box", sch.graphic_shown?.[g.id] ?? g.shape.text, page, ...xy(at)]);
        }
        break;
      case "labels":
        for (const l of sch.labels) {
          const name = unescapeString(l.net);
          if (query !== "" && !matches(name, query)) continue;
          row(l.id, l.at, [l.scope === "global" ? "Global" : l.scope === "hierarchical" ? "Hierarchical" : "Local", name, page, ...xy(l.at)]);
        }
        // a directive label's text is empty: only an empty query lists it
        for (const g of sch.graphics ?? []) {
          if (g.shape.type !== "directive" || query !== "") continue;
          const at: [number, number] = [g.shape.at.x, g.shape.at.y];
          row(g.id, at, ["Directive", "", page, ...xy(at)]);
        }
        break;
      case "groups":
        for (const g of sch.groups ?? []) {
          if (query !== "" && !matches(unescapeString(g.name), query)) continue;
          const at = c.groupCentre?.(sheet, g.id) ?? ([0, 0] as const);
          row(g.id, at, [g.name, page, ...xy(at)]);
        }
        break;
    }
  }
  return rows;
}

/** `SCH_SEARCH_HANDLER::Sort`: by `col` (none: the first column), the cells by `StrNumCmp( .., true )`; stable. */
export function sortSchSearchRows(rows: readonly SchSearchRow[], col: number | null, ascending: boolean): SchSearchRow[] {
  const c = Math.max(0, col ?? 0);
  const cmp = (a: SchSearchRow, b: SchSearchRow) => strNumCmp(a.cells[c] ?? "", b.cells[c] ?? "", true);
  return [...rows].sort((a, b) => (ascending ? cmp(a, b) : cmp(b, a)));
}

/** `SCH_SEARCH_HANDLER::SelectItems`: the sheet to go to (`changeSheet`) when every selected row is on one sheet, else null (nothing is selected then). */
export function schSearchTarget(rows: readonly SchSearchRow[]): { sheet: string[]; items: string[] } | null {
  if (rows.length === 0) return null;
  const first = rows[0]!.sheet.join("/");
  if (!rows.every((r) => r.sheet.join("/") === first)) return null;
  return { sheet: rows[0]!.sheet, items: [...new Set(rows.map((r) => r.item))] };
}

/** `RefreshColumnNames`' widths over the list's width, as the board editor's pane lays them out. */
export function schSearchColumnWidths(tab: SchSearchTabId, listWidth: number): number[] {
  const unit = Math.floor(Math.max(0, listWidth) / 10);
  return (SCH_SEARCH_TABS.find((t) => t.id === tab)?.columns ?? []).map((col) => unit * col.proportion);
}
