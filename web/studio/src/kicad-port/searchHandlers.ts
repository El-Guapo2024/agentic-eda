// The board editor's Search panel (`PCB_SEARCH_PANE`, pcbnew/widgets/pcb_search_pane.cpp, and its handlers, pcbnew/widgets/search_handlers.cpp at 8303b2ad, over
// common/widgets/search_pane.cpp and search_pane_tab.cpp): the seven tabs, what each finds for a query, its columns and cells, its sort, and what a selection or an
// activation of its rows does.
//
//   * Footprints (`FOOTPRINT_SEARCH_HANDLER`): every footprint whose fields match (`PCB_FIELD::Matches`: a hidden field only with "Search Hidden Fields"), or, with
//     "Search Metadata", its library link, description or keywords (`FOOTPRINT::Matches`). Reference, Value, Layer, X, Y, Library Link, Library Description.
//   * Zones (`ZONE_SEARCH_HANDLER`): by name (`ZONE::Matches`). Name, Net, Layer (every layer of the zone, in UI order, joined by ","), Priority, X, Y (the first
//     corner, `ZONE::GetPosition`), Area (the filled area, `GetFilledArea`, or a rule area's outline area).
//   * Nets (`NETS_SEARCH_HANDLER`): by the stored name (`NETINFO_ITEM::Matches( GetNetname() )`); the unconnected net is "No Net". Name, Class.
//   * Ratsnest (`RATSNEST_SEARCH_HANDLER`): the matching nets that have an unrouted connection (`RN_NET::GetEdges()` not empty). Name, Class.
//   * Text (`TEXT_SEARCH_HANDLER`): the board's texts, text boxes and dimensions (`board->Drawings()` of `PCB_TEXT_T`, `PCB_TEXTBOX_T`, a `PCB_DIMENSION_T`). Type,
//     Text, Layer, X, Y.
//   * Groups (`GROUP_SEARCH_HANDLER`): the groups but the generators, by name. Type, Name, X, Y (the centre of its box, `PCB_GROUP::GetPosition`).
//   * Drills (`DRILL_SEARCH_HANDLER`): the holes of the pads and the vias, one row per kind of hole (`DRILL_LINE_ITEM`: size, shape, plated, pad or via, the
//     layers), with its count; a row matches when any of its cells holds the query, lower-cased. Count, Shape, X Size, Y Size, Plated, Via/Pad, Start Layer, Stop Layer.
//
// Matching is `EDA_ITEM::Matches` in `EDA_SEARCH_MATCH_MODE::PERMISSIVE` (kicad-port/edaPatternMatch.ts); an empty query lists everything. After each search the
// rows are sorted by the tab's sort column (`SEARCH_PANE_TAB::Search` -> `SEARCH_PANE_LISTVIEW::Sort`; none chosen is the first column, ascending): the cells by
// `StrNumCmp( .., true )` (`PCB_SEARCH_HANDLER::Sort`), the drills by `DRILL_LINE_ITEM::COMPARE` on their values.
//
// The X and Y cells are `MessageTextFromCoord( .., ABS_X_COORD / ABS_Y_COORD )`: from the origin Preferences > PCB Editor > Origins & Axes chooses, each axis
// either way (`SearchContext.xf`, kicad-port/originTransforms.ts; absent: the page origin, Y down).
// The nets are listed by name, the unconnected net first: KiCad's net-code order for the board the studio writes (it numbers the nets by name).
//
// Pure: no store, no DOM. Unit-tested in searchHandlers.test.ts.
import type { BoardState, Part, Pad } from "../api/types";
import { messageTextFromValue, type MessageUnits } from "./messageText";
import { permissiveMatches } from "./edaPatternMatch";
import { unescapeString } from "./schNetName";
import { strNumCmp } from "./schSheetPins";
import { padIds } from "./pcbItems";
import { zoneLayers } from "./zoneLayers";
import { toDisplay, type CoordType, type OriginTransforms } from "./originTransforms";

export type SearchTabId = "footprints" | "zones" | "nets" | "ratsnest" | "text" | "groups" | "drills";

/** `SEARCH_HANDLER::m_columns`: the name, the width in tenths of the pane, the alignment. */
export interface SearchColumn {
  name: string;
  proportion: number;
  align: "left" | "center" | "right";
}

/** The tabs in the order `PCB_SEARCH_PANE` adds them, each with its name (`_HKI( "Footprints" )` ...) and columns. */
export const SEARCH_TABS: readonly { id: SearchTabId; name: string; columns: readonly SearchColumn[] }[] = [
  {
    id: "footprints",
    name: "Footprints",
    columns: [
      { name: "Reference", proportion: 2, align: "left" },
      { name: "Value", proportion: 6, align: "left" },
      { name: "Layer", proportion: 2, align: "center" },
      { name: "X", proportion: 3, align: "center" },
      { name: "Y", proportion: 3, align: "center" },
      { name: "Library Link", proportion: 8, align: "left" },
      { name: "Library Description", proportion: 10, align: "left" },
    ],
  },
  {
    id: "zones",
    name: "Zones",
    columns: [
      { name: "Name", proportion: 6, align: "left" },
      { name: "Net", proportion: 6, align: "left" },
      { name: "Layer", proportion: 3, align: "center" },
      { name: "Priority", proportion: 2, align: "center" },
      { name: "X", proportion: 3, align: "center" },
      { name: "Y", proportion: 3, align: "center" },
      { name: "Area", proportion: 3, align: "right" },
    ],
  },
  { id: "nets", name: "Nets", columns: [{ name: "Name", proportion: 6, align: "left" }, { name: "Class", proportion: 6, align: "left" }] },
  { id: "ratsnest", name: "Ratsnest", columns: [{ name: "Name", proportion: 6, align: "left" }, { name: "Class", proportion: 6, align: "left" }] },
  {
    id: "text",
    name: "Text",
    columns: [
      { name: "Type", proportion: 2, align: "left" },
      { name: "Text", proportion: 12, align: "left" },
      { name: "Layer", proportion: 3, align: "center" },
      { name: "X", proportion: 3, align: "center" },
      { name: "Y", proportion: 3, align: "center" },
    ],
  },
  {
    id: "groups",
    name: "Groups",
    columns: [
      { name: "Type", proportion: 2, align: "left" },
      { name: "Name", proportion: 6, align: "left" },
      { name: "X", proportion: 3, align: "center" },
      { name: "Y", proportion: 3, align: "center" },
    ],
  },
  {
    id: "drills",
    name: "Drills",
    columns: [
      { name: "Count", proportion: 2, align: "right" },
      { name: "Shape", proportion: 3, align: "left" },
      { name: "X Size", proportion: 3, align: "center" },
      { name: "Y Size", proportion: 3, align: "center" },
      { name: "Plated", proportion: 2, align: "center" },
      { name: "Via/Pad", proportion: 2, align: "center" },
      { name: "Start Layer", proportion: 4, align: "center" },
      { name: "Stop Layer", proportion: 4, align: "center" },
    ],
  },
];

/** `APP_SETTINGS_BASE::SEARCH_PANE`, with its defaults (app_settings.cpp: `selection_zoom` PAN, `search_hidden_fields` true, `search_metadata` false). */
export interface SearchPaneSettings {
  selectionZoom: "none" | "pan" | "zoom";
  searchHiddenFields: boolean;
  searchMetadata: boolean;
}

export const DEFAULT_SEARCH_PANE_SETTINGS: SearchPaneSettings = { selectionZoom: "pan", searchHiddenFields: true, searchMetadata: false };

export function parseSearchPaneSettings(raw: unknown): SearchPaneSettings {
  const d = DEFAULT_SEARCH_PANE_SETTINGS;
  if (!raw || typeof raw !== "object") return d;
  const o = raw as Record<string, unknown>;
  return {
    selectionZoom: o.selectionZoom === "none" || o.selectionZoom === "pan" || o.selectionZoom === "zoom" ? o.selectionZoom : d.selectionZoom,
    searchHiddenFields: typeof o.searchHiddenFields === "boolean" ? o.searchHiddenFields : d.searchHiddenFields,
    searchMetadata: typeof o.searchMetadata === "boolean" ? o.searchMetadata : d.searchMetadata,
  };
}

/**
 * `SEARCH_PANE_MENU`'s check items: Zoom to Selection and Pan to Selection are one setting (checking one unchecks the other, unchecking it leaves NONE), Search
 * Hidden Fields and Search Metadata are their own.
 */
export function toggleSearchSetting(s: SearchPaneSettings, id: "zoom" | "pan" | "hidden" | "metadata"): SearchPaneSettings {
  switch (id) {
    case "zoom":
      return { ...s, selectionZoom: s.selectionZoom === "zoom" ? "none" : "zoom" };
    case "pan":
      return { ...s, selectionZoom: s.selectionZoom === "pan" ? "none" : "pan" };
    case "hidden":
      return { ...s, searchHiddenFields: !s.searchHiddenFields };
    case "metadata":
      return { ...s, searchMetadata: !s.searchMetadata };
  }
}

/** What the handlers read beside the board: each copper zone's filled area (nm², the fill's `area_nm2`), the nets that have a ratsnest line, each group's box centre, and each net's class. */
export interface SearchContext {
  units: MessageUnits;
  settings: SearchPaneSettings;
  filledArea?: ReadonlyMap<string, number>;
  ratsnestNets?: ReadonlySet<string>;
  groupCentre?: ReadonlyMap<string, readonly [number, number]>;
  /** `NETINFO_ITEM::GetNetClass()->GetName()`. */
  netClassName?: (net: string) => string;
  /** The board editor's origin transforms (`m_frame->GetOriginTransforms()`, Origins & Axes), which the X and Y cells go through; absent: none. */
  xf?: OriginTransforms;
}

/** A hole kind of the Drills tab (`DRILL_LINE_ITEM`): its sizes, shape, plating, whether it is a pad's, its copper layers, and how many there are. */
export interface DrillEntry {
  xSize: number;
  ySize: number;
  shape: "circle" | "oblong";
  isPlated: boolean;
  isPad: boolean;
  startLayer: string | null;
  stopLayer: string | null;
  qty: number;
  /** The pads (`REF.NUM[#k]`) and vias of this kind: what selecting the row selects. */
  items: string[];
}

/** A row of a tab: the item it stands for (its id: a footprint's reference, a zone's, a net's name, a drill kind's first item), its cells, and what selecting it selects or highlights. */
export interface SearchRow {
  id: string;
  cells: string[];
  /** The board items a selection of the row selects (`ACTIONS::selectItems`); empty for a net row. */
  select: string[];
  /** The net a selection of the row highlights (Nets, Ratsnest). */
  net?: string;
  /** Drills only: the values the drill comparator sorts by. */
  drill?: DrillEntry;
}

/** `PCB_BASE_FRAME::MessageTextFromCoord( aValue, aCoordType )`: `MessageTextFromValue( m_originTransforms.ToDisplay( aValue, aCoordType ) )`. */
const coord = (v: number, c: SearchContext, type: CoordType) => messageTextFromValue(c.xf ? toDisplay(c.xf, v, type) : v, c.units);

/** `PCB_FIELD::Matches`: a field that is not visible only with "Search Hidden Fields"; its text unescaped. */
function fieldsOf(p: Part): { text: string; visible: boolean }[] {
  if (p.fields && p.fields.length > 0) return p.fields.map((f) => ({ text: f.text, visible: f.visible }));
  return [{ text: p.ref, visible: true }, ...(p.value !== null ? [{ text: p.value, visible: true }] : [])];
}

/**
 * `GetFPID().Format()` (the Library Link cell) and `GetFPIDAsString()` (what Search Metadata matches): the `Lib:Name` the board file gives the footprint (`lib_link`:
 * a built-in package's installed KiCad footprint, a name with no library bare); from a backend that does not send it, the part's footprint name.
 */
function libraryLink(p: Part): string {
  return p.lib_link ?? p.footprint ?? "";
}

function footprintRows(board: BoardState, query: string, c: SearchContext): SearchRow[] {
  const rows: SearchRow[] = [];
  for (const p of board.parts) {
    if (!p.placed || !p.at) continue; // `board->Footprints()`: only what is on the board
    let found = query === "";
    // `FOOTPRINT::Matches`: the library link, description and keywords, with "Search Metadata"
    if (!found && c.settings.searchMetadata) found = [libraryLink(p), p.lib_description ?? "", p.keywords ?? ""].some((t) => permissiveMatches(t, query));
    if (!found) found = fieldsOf(p).some((f) => (f.visible || c.settings.searchHiddenFields) && permissiveMatches(unescapeString(f.text), query));
    if (!found) continue;
    rows.push({
      id: p.ref,
      cells: [p.ref, unescapeString(p.value ?? ""), p.side === "bottom" ? "B.Cu" : "F.Cu", coord(p.at[0], c, "abs_x"), coord(p.at[1], c, "abs_y"), libraryLink(p), p.lib_description ?? ""],
      select: [p.ref],
    });
  }
  return rows;
}

/** `SHAPE_POLY_SET::Area` of an outline and its holes (shoelace, holes taken away). */
function polygonArea(outline: readonly (readonly [number, number])[], holes: readonly (readonly (readonly [number, number])[])[] = []): number {
  const ring = (pts: readonly (readonly [number, number])[]) => {
    let a = 0;
    for (let i = 0; i < pts.length; i++) {
      const [x1, y1] = pts[i]!;
      const [x2, y2] = pts[(i + 1) % pts.length]!;
      a += x1 * y2 - x2 * y1;
    }
    return Math.abs(a) / 2;
  };
  return Math.max(0, ring(outline) - holes.reduce((s, h) => s + ring(h), 0));
}

function zoneRows(board: BoardState, query: string, c: SearchContext): SearchRow[] {
  const rows: SearchRow[] = [];
  for (const z of board.routing?.zones ?? []) {
    if (query !== "" && !permissiveMatches(z.name ?? "", query)) continue;
    const first = z.outline[0] ?? [0, 0];
    // "Make sure we don't show layers from Rule Areas that aren't actually on the board": the zone's layers that the board has
    const layers = zoneLayers(z).filter((l) => !/\.Cu$/.test(l) || board.layers.includes(l));
    const area = z.is_rule_area ? polygonArea(z.outline, z.holes) : (c.filledArea?.get(z.id) ?? 0);
    rows.push({
      id: z.id,
      cells: [z.name ?? "", unescapeString(z.net), layers.join(","), String(z.priority), coord(first[0], c, "abs_x"), coord(first[1], c, "abs_y"), messageTextFromValue(area, c.units, true, "area")],
      select: [z.id],
    });
  }
  return rows;
}

/** Every net of the board (`BOARD::GetNetInfo()`): the unconnected net first, then the named ones, by name. */
export function searchNets(board: BoardState, extra: Iterable<string> = []): string[] {
  const nets = new Set<string>();
  for (const p of board.parts) if (p.placed) for (const pad of p.pads ?? []) if (pad.net) nets.add(pad.net);
  for (const t of board.routing?.tracks ?? []) if (t.net) nets.add(t.net);
  for (const v of board.routing?.vias ?? []) if (v.net) nets.add(v.net);
  for (const z of board.routing?.zones ?? []) if (z.net && !z.is_rule_area) nets.add(z.net);
  for (const n of extra) if (n) nets.add(n);
  return ["", ...[...nets].sort()];
}

function netRow(net: string, c: SearchContext): SearchRow {
  // `getResultCell`: "No Net" and no class for the net of code 0
  if (net === "") return { id: "", cells: ["No Net", ""], select: [], net: "" };
  return { id: net, cells: [unescapeString(net), c.netClassName?.(net) ?? "Default"], select: [], net };
}

function netRows(board: BoardState, query: string, c: SearchContext, extraNets: Iterable<string>): SearchRow[] {
  // `aQuery.IsEmpty() || net->Matches( frp, nullptr )`: the stored name
  return searchNets(board, extraNets).filter((n) => query === "" || permissiveMatches(n, query)).map((n) => netRow(n, c));
}

function ratsnestRows(board: BoardState, query: string, c: SearchContext, extraNets: Iterable<string>): SearchRow[] {
  // no empty-query shortcut here: an empty pattern is found by the substring matcher, so it matches every net
  return searchNets(board, extraNets)
    .filter((n) => (query === "" || permissiveMatches(n, query)) && (c.ratsnestNets?.has(n) ?? false))
    .map((n) => netRow(n, c));
}

function textRows(board: BoardState, query: string, c: SearchContext): SearchRow[] {
  const d = board.drawings;
  if (!d) return [];
  const rows: SearchRow[] = [];
  const add = (id: string, type: string, text: string, layer: string, x: number, y: number) => {
    // `PCB_TEXT::Matches` / `PCB_TEXTBOX::Matches`: `UnescapeString( GetText() )`; a dimension is a `PCB_TEXT`
    const shown = unescapeString(text);
    if (query !== "" && !permissiveMatches(shown, query)) return;
    rows.push({ id, cells: [type, shown, layer, coord(x, c, "abs_x"), coord(y, c, "abs_y")], select: [id] });
  };
  for (const t of d.texts) add(t.id, "Text", t.content, t.layer, t.x, t.y);
  for (const t of d.text_boxes ?? []) add(t.id, "Textbox", t.text, t.layer, t.start[0], t.start[1]);
  // `PCB_DIMENSION_BASE::GetPosition` is its start
  for (const dim of d.dimensions) add(dim.id, "Dimension", dim.text, dim.layer, dim.start[0], dim.start[1]);
  return rows;
}

function groupRows(board: BoardState, query: string, c: SearchContext): SearchRow[] {
  const rows: SearchRow[] = [];
  for (const g of board.drawings?.groups ?? []) {
    if (g.generator) continue; // "Skip generators, they are for internal use, not user-facing grouping"
    if (query !== "" && !permissiveMatches(unescapeString(g.name), query)) continue;
    const at = c.groupCentre?.get(g.id) ?? [0, 0];
    rows.push({ id: g.id, cells: ["Group", g.name, coord(at[0], c, "abs_x"), coord(at[1], c, "abs_y")], select: [g.id] });
  }
  return rows;
}

/** The board's copper layers in KiCad's layer-id order (F.Cu 0, B.Cu 2, In1.Cu 4, ...): what `DRILL_LINE_ITEM::COMPARE` compares a start or stop layer by. */
function layerId(layer: string | null): number {
  if (layer === null) return -1; // UNDEFINED_LAYER
  if (layer === "F.Cu") return 0;
  if (layer === "B.Cu") return 2;
  const n = /^In(\d+)\.Cu$/.exec(layer);
  return n ? 2 + 2 * Number(n[1]) : 1000;
}

/** A pad's copper layers, front to back (`GetLayerSet().CuStack()`): its own list (in the footprint's frame, so a back footprint's are flipped), else all of them for a plated hole and none for an unplated one. */
function padCopper(pad: Pad, side: Part["side"], copper: readonly string[]): string[] {
  const flip = (l: string) => (side === "bottom" ? (l.startsWith("F.") ? "B." + l.slice(2) : l.startsWith("B.") ? "F." + l.slice(2) : l) : l);
  const own = pad.layers;
  if (!own || own.length === 0) return pad.kind === "non_plated_hole" ? [] : [...copper];
  if (own.includes("*.Cu")) return [...copper];
  const set = new Set(own.map(flip));
  return copper.filter((l) => set.has(l));
}

/** `DRILL_SEARCH_HANDLER::Search`'s collection: the holes of the pads, then of the vias, one entry per kind (`addEntryOrIncrement`). */
export function drillEntries(board: BoardState): DrillEntry[] {
  // the copper stack, front to back (`CuStack()`: F.Cu, In1.Cu .. In30.Cu, B.Cu)
  const stackOrder = (l: string) => (l === "F.Cu" ? -1 : l === "B.Cu" ? 1e6 : layerId(l));
  const copper = [...board.layers].sort((a, b) => stackOrder(a) - stackOrder(b));
  const entries: DrillEntry[] = [];
  const add = (e: Omit<DrillEntry, "qty" | "items">, item: string) => {
    const same = entries.find((g) => g.xSize === e.xSize && g.ySize === e.ySize && g.shape === e.shape && g.isPlated === e.isPlated && g.isPad === e.isPad && g.startLayer === e.startLayer && g.stopLayer === e.stopLayer);
    if (same) {
      same.qty++;
      same.items.push(item);
    } else entries.push({ ...e, qty: 1, items: [item] });
  };
  for (const p of board.parts) {
    if (!p.placed) continue;
    const ids = padIds(p);
    (p.pads ?? []).forEach((pad, i) => {
      const slot = pad.slot ?? null;
      const xs = slot ? slot[0] : (pad.drill ?? 0);
      const ys = slot ? slot[1] : (pad.drill ?? 0);
      if (!(xs > 0) || !(ys > 0)) return; // `!pad->HasHole()`, a hole of no size
      const cu = padCopper(pad, p.side, copper);
      add({ xSize: xs, ySize: ys, shape: slot ? "oblong" : "circle", isPlated: pad.kind !== "non_plated_hole", isPad: true, startLayer: cu[0] ?? null, stopLayer: cu[cu.length - 1] ?? null }, pad.id ?? ids[i]!);
    });
  }
  for (const v of board.routing?.vias ?? []) {
    if (!(v.drill > 0)) continue;
    // `TopLayer()` / `BottomLayer()`: the via's two layers in stack order, the front one first
    const [a, b] = stackOrder(v.from) <= stackOrder(v.to) ? [v.from, v.to] : [v.to, v.from];
    add({ xSize: v.drill, ySize: v.drill, shape: "circle", isPlated: true, isPad: false, startLayer: a, stopLayer: b }, v.id);
  }
  // `DRILL_LINE_ITEM::COMPARE( COL_COUNT, false )`: the most numerous first
  return entries.sort((x, y) => y.qty - x.qty);
}

/** `DRILL_SEARCH_HANDLER::cellText`. */
export function drillCell(e: DrillEntry, col: number, units: MessageUnits): string {
  switch (col) {
    case 0:
      return String(e.qty);
    case 1:
      return e.shape === "circle" ? "Round" : "Slot";
    case 2:
      return messageTextFromValue(e.xSize, units);
    case 3:
      return messageTextFromValue(e.ySize, units);
    case 4:
      return e.isPlated ? "PTH" : "NPTH";
    case 5:
      return e.isPad ? "Pad" : "Via";
    case 6:
      return e.startLayer ?? "N/A";
    case 7:
      return e.stopLayer ?? "N/A";
    default:
      return "";
  }
}

function drillRows(board: BoardState, query: string, c: SearchContext): SearchRow[] {
  const q = query.toLowerCase();
  return drillEntries(board)
    .filter((e) => q === "" || [0, 1, 2, 3, 4, 5, 6, 7].some((col) => drillCell(e, col, c.units).toLowerCase().includes(q))) // `rowMatchesQuery`
    .map((e) => ({ id: e.items[0]!, cells: [0, 1, 2, 3, 4, 5, 6, 7].map((col) => drillCell(e, col, c.units)), select: e.items, drill: e }));
}

/** `SEARCH_HANDLER::Search` of a tab: its rows for `query`, in the order the handler finds them. `extraNets` are nets the board has that no item names. */
export function searchTab(tab: SearchTabId, board: BoardState, query: string, c: SearchContext, extraNets: Iterable<string> = []): SearchRow[] {
  switch (tab) {
    case "footprints":
      return footprintRows(board, query, c);
    case "zones":
      return zoneRows(board, query, c);
    case "nets":
      return netRows(board, query, c, extraNets);
    case "ratsnest":
      return ratsnestRows(board, query, c, extraNets);
    case "text":
      return textRows(board, query, c);
    case "groups":
      return groupRows(board, query, c);
    case "drills":
      return drillRows(board, query, c);
  }
}

/** `DRILL_LINE_ITEM::COMPARE` on one column, as a three-way comparison. */
function compareDrills(a: DrillEntry, b: DrillEntry, col: number): number {
  const shape = (e: DrillEntry) => (e.shape === "circle" ? 1 : 2); // PAD_DRILL_SHAPE::CIRCLE, OBLONG
  switch (col) {
    case 0:
      return a.qty - b.qty;
    case 1:
      return shape(a) - shape(b);
    case 2:
      return a.xSize - b.xSize;
    case 3:
      return a.ySize - b.ySize;
    case 4:
      return Number(a.isPlated) - Number(b.isPlated);
    case 5:
      return Number(a.isPad) - Number(b.isPad);
    case 6:
      return layerId(a.startLayer) - layerId(b.startLayer);
    case 7:
      return layerId(a.stopLayer) - layerId(b.stopLayer);
    default:
      return 0;
  }
}

/**
 * `SEARCH_HANDLER::Sort( aCol, aAscending )`: by `col` (none: the first column, "Provide a stable order by sorting on first column if no sort column provided"),
 * the cells by `StrNumCmp( .., true )`, the drills by their values. Stable: rows that compare equal keep the order they were found in.
 */
export function sortSearchRows(rows: readonly SearchRow[], tab: SearchTabId, col: number | null, ascending: boolean): SearchRow[] {
  const c = Math.max(0, col ?? 0);
  const cmp = (a: SearchRow, b: SearchRow) => (tab === "drills" && a.drill && b.drill ? compareDrills(a.drill, b.drill, c) : strNumCmp(a.cells[c] ?? "", b.cells[c] ?? "", true));
  return [...rows].sort((a, b) => (ascending ? cmp(a, b) : cmp(b, a)));
}

/** `SEARCH_PANE_LISTVIEW::OnColClicked`: the same column again turns the order round; another sorts ascending by it. */
export function clickSearchColumn(sort: { col: number | null; ascending: boolean }, col: number): { col: number; ascending: boolean } {
  return sort.col === col ? { col, ascending: !sort.ascending } : { col, ascending: true };
}

/**
 * `CopySelectionToClipboard`: the selected rows' cells, tab-separated, a line each, in the columns' own order (`GetItemText( row, col )`, whatever order they are
 * shown in), leaving out a column dragged to no width (`if( GetColumnWidth( col ) <= 0 ) continue;`). `widths` are the columns' widths; none: every column.
 */
export function searchRowsText(rows: readonly SearchRow[], widths?: readonly number[]): string {
  return rows.map((r) => r.cells.filter((_, i) => !widths || (widths[i] ?? 1) > 0).join("\t") + "\n").join("");
}

// ------------------------------------------------------------------------------------------------------------------------------ the columns on screen

/**
 * The columns of a tab in the order they are shown: `order` (model column indexes) when the person has dragged a heading to another place -- a `wxListView` in
 * report mode lets the native list's headers be dragged (wxMSW's `LVS_EX_HEADERDRAGDROP`) and keeps the model's indexes, so the sort and Copy still go by the
 * model's column -- else the handler's own order. A stale or partial `order` falls back to the handler's.
 */
export function searchColumnOrder(count: number, order?: readonly number[]): number[] {
  const natural = Array.from({ length: count }, (_, i) => i);
  if (!order || order.length !== count || new Set(order).size !== count || order.some((i) => !(i >= 0 && i < count))) return natural;
  return [...order];
}

/** A heading dragged to another place: model column `col` put before the column now shown at display position `to` (the end for `to` past the last). */
export function moveSearchColumn(count: number, order: readonly number[] | undefined, col: number, to: number): number[] {
  const out = searchColumnOrder(count, order);
  const from = out.indexOf(col);
  if (from < 0) return out;
  const at = Math.max(0, Math.min(out.length, to));
  if (at === from || at === from + 1) return out;
  out.splice(from, 1);
  out.splice(at > from ? at - 1 : at, 0, col);
  return out;
}

/**
 * `RefreshColumnNames`' widths: `GetClientSize().GetWidth() / 10` times each column's proportion. The studio lays the proportions out over the list's width as they
 * are (`listWidth` px; the columns of a tab whose proportions add up to more than ten then run past the list, which scrolls sideways, as KiCad's do).
 */
export function searchColumnWidths(tab: SearchTabId, listWidth: number): number[] {
  const unit = Math.floor(Math.max(0, listWidth) / 10);
  return (SEARCH_TABS.find((t) => t.id === tab)?.columns ?? []).map((c) => unit * c.proportion);
}

/** The list's own margins round a cell's text (the report-mode list's `wxLIST_AUTOSIZE`: the text and a little room either side). */
const SEARCH_CELL_MARGINS = 12;

/**
 * A double click on a heading's edge: the native list sizes the column to its widest text (`LVSCW_AUTOSIZE` on Windows), here the heading's too so it is never
 * cut. `measure` is the text's width in the list's font.
 */
export function autosizeSearchColumn(tab: SearchTabId, col: number, rows: readonly SearchRow[], measure: (text: string) => number): number {
  const heading = SEARCH_TABS.find((t) => t.id === tab)?.columns[col]?.name ?? "";
  let max = measure(heading) + SEARCH_CELL_MARGINS + 12; // the sort arrow's room on the heading
  for (const r of rows) max = Math.max(max, measure(r.cells[col] ?? "") + SEARCH_CELL_MARGINS);
  return Math.ceil(max);
}

/** `NETINFO_ITEM::GetNetClass()->GetName()` from the classes a net is in (`NET_SETTINGS::GetEffectiveNetClass`): the Default class for none, the class for one that sets every parameter, else the composite "Effective for net: %s". */
export function effectiveNetClassName(net: string, effective: string): string {
  const names = effective.split(",");
  return names.length === 1 ? names[0]! : `Effective for net: ${net}`;
}
