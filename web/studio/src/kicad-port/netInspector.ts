// The Net Inspector (`PCB_NET_INSPECTOR_PANEL`, pcbnew/widgets/pcb_net_inspector_panel.cpp and pcb_net_inspector_panel_data_model.h at 8303b2ad): its
// columns, its filter, its groups, its sort, its menus, its net edits' checks and its report.
//
// The numbers are the backend's (`GET /api/net_inspector`, crates/cli/src/net_inspector_api.rs: `calculateNets` and
// `LENGTH_DELAY_CALCULATION::CalculateLengthDetails`, the lengths and the delays); this file is what the panel does with them:
//   * the columns (`buildColumns`): Name, Netclass, Total Length, Via Count, Via Length, Track Length, Die Length, Pad Count and one per copper layer; with
//     "Show Time Domain Details" the length columns are Total Delay, Via Delay, Track Delay, Die Delay and each layer's delay (`DATA_MODEL::GetValue`'s
//     `m_show_time_domain_details` branches); lengths by `MessageTextFromValue` (`formatLength`), delays in picoseconds (`formatDelay`, `EDA_DATA_TYPE::TIME`);
//   * which nets are listed (`netFilterMatches`): never the unconnected net; the search text matched, upper-cased, as a substring of the net's name ("Filter
//     by Net Name") or its class's ("Filter by Netclass"); a net named `unconnected-(...` only with "Show Unconnected Nets"; a net with no pad only with "Show
//     Zero Pad Nets";
//   * the groups (`DATA_MODEL::addCustomGroups`, `addItem`): every custom group rule is a group, shown even when empty ("Custom: <pattern>"); a net goes to the
//     first rule whose `EDA_COMBINED_MATCHER( rule, CTX_NET )` finds its name, else, with "Group by Netclass", to the group of its class ("Netclass: <class>",
//     made when its first net comes); a group's numbers are its nets' sums (`LIST_ITEM::SetParent`'s Add/Sub);
//   * the sort (`DATA_MODEL::Compare`): a group before a net; by the column's value (the name and the class by `ValueStringCompare`, the numbers as numbers),
//     the order the rows came in breaking a tie; a group's children are sorted the same way under it;
//   * the menus: the configure button's (`OnConfigButton`), the rows' (`OnNetsListContextMenu`), and Show / Hide Columns (`generateShowHideColumnMenu`);
//   * the net edits' prompts (`onRenameSelectedNet`, `onAddNet`, `onDeleteSelectedNet`, `onAddGroup`) and their messages, word for word;
//   * Save Net Inspector Report... (`generateReport`): the CSV KiCad writes, `;` after every field, the text columns quoted, the values without their unit label,
//     every column (a hidden one too) and every listed net (the groups left out).
//
// Pure: no store, no DOM. Unit-tested in netInspector.test.ts.
import { valueStringCompare } from "./valueStringCompare";
import { messageTextFromValue, type MessageUnits } from "./messageText";
import { EdaCombinedMatcher } from "./edaPatternMatch";
import { escapeNetname, unescapeString } from "./schNetName";

/** One net's row, as the backend sends it (lengths in nm, delays in attoseconds). */
export interface NetInspectorRow {
  name: string;
  netclass: string;
  /** `NETCLASS::GetName()` of the effective class ("Effective for net: <net>" for a net several classes claim), what the netclass filter matches; absent from an older backend. */
  netclass_name?: string;
  total_length: number;
  via_count: number;
  via_length: number;
  track_length: number;
  die_length: number;
  pad_count: number;
  /** Track length per copper layer (`LENGTH_DELAY_STATS::LayerLengths`), nm; a layer the net has no track on is absent. */
  layer_lengths?: Record<string, number>;
  /** The time domain (`WITH_DELAY_DETAIL`), attoseconds; absent from an older backend (read as 0). */
  total_delay?: number;
  via_delay?: number;
  track_delay?: number;
  die_delay?: number;
  layer_delays?: Record<string, number>;
}

/** The fixed columns' model ids (`COLUMN_NAME` .. `COLUMN_PAD_COUNT`), or `layer:<name>` for one copper layer's. */
export type NetInspectorColumn = "name" | "netclass" | "total_length" | "via_count" | "via_length" | "track_length" | "die_length" | "pad_count" | `layer:${string}`;

export interface NetInspectorColumnDesc {
  key: NetInspectorColumn;
  label: string;
  csv: string;
  units: boolean;
}

/** `buildColumns`: (key, display name, CSV name, has units), in the length domain. */
export const NET_INSPECTOR_COLUMNS: readonly NetInspectorColumnDesc[] = [
  { key: "name", label: "Name", csv: "Net Name", units: false },
  { key: "netclass", label: "Netclass", csv: "Netclass", units: false },
  { key: "total_length", label: "Total Length", csv: "Net Length", units: true },
  { key: "via_count", label: "Via Count", csv: "Via Count", units: false },
  { key: "via_length", label: "Via Length", csv: "Via Length", units: true },
  { key: "track_length", label: "Track Length", csv: "Track Length", units: true },
  { key: "die_length", label: "Die Length", csv: "Die Length", units: true },
  { key: "pad_count", label: "Pad Count", csv: "Pad Count", units: false },
];

/** `buildColumns`' `m_showTimeDomainDetails` branches: the four length columns become delays. */
const TIME_DOMAIN_COLUMNS: Readonly<Partial<Record<NetInspectorColumn, { label: string; csv: string }>>> = {
  total_length: { label: "Total Delay", csv: "Net Delay" },
  via_length: { label: "Via Delay", csv: "Via Delay" },
  track_length: { label: "Track Delay", csv: "Track Delay" },
  die_length: { label: "Die Delay", csv: "Die Delay" },
};

const isLayerColumn = (c: NetInspectorColumn): c is `layer:${string}` => c.startsWith("layer:");

/**
 * Every column of the panel: the fixed ones, then one per enabled copper layer (`for( PCB_LAYER_ID layer : m_board->GetEnabledLayers().Seq() )`, in
 * layer-id order: F.Cu, B.Cu, then the inner layers), named by the layer, a length (or a delay) in the user's units.
 */
export function netInspectorColumns(copperLayers: readonly string[], timeDomain = false): NetInspectorColumnDesc[] {
  const order = (l: string) => (l === "F.Cu" ? -2 : l === "B.Cu" ? -1 : Number(/^In(\d+)\.Cu$/.exec(l)?.[1] ?? 1000));
  const layers = [...copperLayers].sort((a, b) => order(a) - order(b));
  const fixed = NET_INSPECTOR_COLUMNS.map((c) => (timeDomain && TIME_DOMAIN_COLUMNS[c.key] ? { ...c, ...TIME_DOMAIN_COLUMNS[c.key]! } : c));
  return [...fixed, ...layers.map((l): NetInspectorColumnDesc => ({ key: `layer:${l}`, label: l, csv: l, units: true }))];
}

// ------------------------------------------------------------------------------------------------------------------------------------------- settings

/** The filter part of `PANEL_NET_INSPECTOR_SETTINGS` (what `netFilterMatches` and `calculateNets` read). */
export interface NetInspectorOptions {
  filter: string;
  filterByNetName: boolean;
  filterByNetclass: boolean;
  showZeroPadNets: boolean;
  showUnconnectedNets: boolean;
}

export const DEFAULT_NET_INSPECTOR_OPTIONS: NetInspectorOptions = { filter: "", filterByNetName: true, filterByNetclass: true, showZeroPadNets: false, showUnconnectedNets: false };

/**
 * `PANEL_NET_INSPECTOR_SETTINGS` (project_local_settings.cpp's `net_inspector_panel.*`, with its defaults): the filter, the grouping, the time domain, the sort,
 * the hidden columns (by column, where KiCad keeps a flag per column position), the expanded groups and the custom group rules. KiCad keeps them in the
 * project's `.kicad_prl`; the studio keeps them in the person's browser (state/netInspectorSettings.ts). Column order and widths are not kept: the columns stay
 * in KiCad's order and size to their contents.
 */
export interface NetInspectorSettings extends NetInspectorOptions {
  groupByNetclass: boolean;
  showTimeDomainDetails: boolean;
  /** `sorting_column`: the column the list is sorted by, `null` for none (-1: the Name column then, ascending). */
  sortingColumn: NetInspectorColumn | null;
  sortAscending: boolean;
  /** `col_hidden`: the columns hidden from the list. */
  hiddenColumns: NetInspectorColumn[];
  /** `expanded_rows`: the names of the groups that are open. */
  expandedRows: string[];
  /** `custom_group_rules`: the patterns of the custom groups, in the order they were added. */
  customGroupRules: string[];
}

export const DEFAULT_NET_INSPECTOR_SETTINGS: NetInspectorSettings = {
  ...DEFAULT_NET_INSPECTOR_OPTIONS,
  groupByNetclass: false,
  showTimeDomainDetails: false,
  sortingColumn: null,
  sortAscending: true,
  hiddenColumns: [],
  expandedRows: [],
  customGroupRules: [],
};

const COLUMN_RE = /^(name|netclass|total_length|via_count|via_length|track_length|die_length|pad_count|layer:.+)$/;
const isColumn = (v: unknown): v is NetInspectorColumn => typeof v === "string" && COLUMN_RE.test(v);
const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

/**
 * Reads stored settings back: anything missing or of the wrong type keeps its default. `buildNetsList` reads every flag but "Show Unconnected Nets", which
 * KiCad saves and never reads back (`m_showUnconnectedNets` starts false in each session), so it is not restored here either; `OnBoardChanged` puts the
 * filter text back.
 */
export function parseNetInspectorSettings(raw: unknown): NetInspectorSettings {
  const d = DEFAULT_NET_INSPECTOR_SETTINGS;
  if (!raw || typeof raw !== "object") return d;
  const o = raw as Record<string, unknown>;
  const bool = (k: keyof NetInspectorSettings, fallback: boolean) => (typeof o[k] === "boolean" ? (o[k] as boolean) : fallback);
  return {
    filter: typeof o.filter === "string" ? o.filter : d.filter,
    filterByNetName: bool("filterByNetName", d.filterByNetName),
    filterByNetclass: bool("filterByNetclass", d.filterByNetclass),
    showZeroPadNets: bool("showZeroPadNets", d.showZeroPadNets),
    showUnconnectedNets: d.showUnconnectedNets,
    groupByNetclass: bool("groupByNetclass", d.groupByNetclass),
    showTimeDomainDetails: bool("showTimeDomainDetails", d.showTimeDomainDetails),
    sortingColumn: isColumn(o.sortingColumn) ? o.sortingColumn : null,
    sortAscending: bool("sortAscending", d.sortAscending),
    hiddenColumns: strings(o.hiddenColumns).filter(isColumn),
    expandedRows: strings(o.expandedRows),
    // "Check that all rows are unique": a pattern stored twice is one group
    customGroupRules: [...new Set(strings(o.customGroupRules).filter((r) => r !== ""))],
  };
}

// ----------------------------------------------------------------------------------------------------------------------------------------- the values

/** The value a column holds for a net row, in the domain shown (a layer column is the row's length or delay on that layer, 0 for none). */
function rowValue(row: NetInspectorRow, column: NetInspectorColumn, timeDomain: boolean): string | number {
  if (isLayerColumn(column)) {
    const layer = column.slice(6);
    return (timeDomain ? row.layer_delays?.[layer] : row.layer_lengths?.[layer]) ?? 0;
  }
  if (timeDomain) {
    if (column === "total_length") return row.total_delay ?? 0;
    if (column === "via_length") return row.via_delay ?? 0;
    if (column === "track_length") return row.track_delay ?? 0;
    if (column === "die_length") return row.die_delay ?? 0;
  }
  return row[column];
}

/** `netFilterMatches` and `calculateNets`'s zero-pad test. */
export function netShown(row: NetInspectorRow, o: NetInspectorOptions): boolean {
  if (row.name === "") return false; // "Never show an unconnected net"
  if (!o.showZeroPadNets && row.pad_count === 0) return false;
  const filter = unescapeString(o.filter).toUpperCase();
  const name = unescapeString(row.name).toUpperCase();
  let matched = filter.length === 0;
  // `netFilterMatches`: `UnescapeString( netClass->GetName() ).Upper()`, the effective class's own name, not the column's human-readable one
  if (!matched && o.filterByNetclass && unescapeString(row.netclass_name ?? row.netclass).toUpperCase().includes(filter)) matched = true;
  if (!matched && o.filterByNetName && name.includes(filter)) matched = true;
  if (matched && !o.showUnconnectedNets) matched = !name.startsWith("UNCONNECTED-(");
  return matched;
}

/** `DATA_MODEL::Compare` of two nets (or two groups) by `column`: the text columns by `ValueStringCompare`, the numbers as numbers; 0 when they are equal. */
function compareValues(x: string | number, y: string | number): number {
  return typeof x === "number" && typeof y === "number" ? x - y : valueStringCompare(String(x), String(y));
}

/** The rows shown, sorted by `column` (the name breaking a tie), with no groups: the numbers as numbers, the Name and Netclass columns by `ValueStringCompare`. */
export function netInspectorRows(rows: readonly NetInspectorRow[], o: NetInspectorOptions, column: NetInspectorColumn = "name", ascending = true, timeDomain = false): NetInspectorRow[] {
  const shown = rows.filter((r) => netShown(r, o));
  const cmp = (a: NetInspectorRow, b: NetInspectorRow): number => {
    const c = compareValues(rowValue(a, column, timeDomain), rowValue(b, column, timeDomain));
    return c !== 0 ? c : valueStringCompare(a.name, b.name);
  };
  shown.sort((a, b) => (ascending ? cmp(a, b) : cmp(b, a)));
  return shown;
}

// ----------------------------------------------------------------------------------------------------------------------------------------- the groups

export type NetInspectorGroupType = "netclass" | "custom";

/** A group row (`LIST_ITEM( aGroupNumber, aGroupName, aGroupType )`): its name, its kind, its nets and their sums. */
export interface NetInspectorGroup {
  type: NetInspectorGroupType;
  /** `GetGroupName()`: the custom rule's pattern, or the class's human-readable name. */
  name: string;
  /** The net rows it holds, sorted as the list is. */
  children: NetInspectorRow[];
  /** The sums of its nets' numbers, as a row (`name` is the group's name, `netclass` empty: `GetNetclassName()` of a group). */
  totals: NetInspectorRow;
}

/** A line of the list: a group (with whether it is open) or a net (`depth` 1 under an open group). */
export type NetInspectorLine = { kind: "group"; group: NetInspectorGroup; expanded: boolean } | { kind: "net"; row: NetInspectorRow; depth: 0 | 1; group: NetInspectorGroup | null };

/** The key a line is selected by: `net:<name>` or `group:<type>:<name>`. */
export function lineKey(line: NetInspectorLine): string {
  return line.kind === "net" ? `net:${line.row.name}` : `group:${line.group.type}:${line.group.name}`;
}

function sumRows(name: string, rows: readonly NetInspectorRow[]): NetInspectorRow {
  const t: NetInspectorRow = { name, netclass: "", total_length: 0, via_count: 0, via_length: 0, track_length: 0, die_length: 0, pad_count: 0, layer_lengths: {}, total_delay: 0, via_delay: 0, track_delay: 0, die_delay: 0, layer_delays: {} };
  for (const r of rows) {
    t.total_length += r.total_length;
    t.via_count += r.via_count;
    t.via_length += r.via_length;
    t.track_length += r.track_length;
    t.die_length += r.die_length;
    t.pad_count += r.pad_count;
    t.total_delay! += r.total_delay ?? 0;
    t.via_delay! += r.via_delay ?? 0;
    t.track_delay! += r.track_delay ?? 0;
    t.die_delay! += r.die_delay ?? 0;
    for (const [l, v] of Object.entries(r.layer_lengths ?? {})) t.layer_lengths![l] = (t.layer_lengths![l] ?? 0) + v;
    for (const [l, v] of Object.entries(r.layer_delays ?? {})) t.layer_delays![l] = (t.layer_delays![l] ?? 0) + v;
  }
  return t;
}

/**
 * The list as `buildNetsList` builds it and the view sorts it: the custom groups (every rule, even with no net), the netclass groups of "Group by Netclass"
 * (one per class that has a listed net), the nets that are in no group; a group before a net, each level sorted by the sorting column (`Name` ascending when none
 * is set), a group's nets under it when it is open (`expandedRows`).
 */
export function netInspectorTree(rows: readonly NetInspectorRow[], s: NetInspectorSettings): NetInspectorLine[] {
  const column = s.sortingColumn ?? "name";
  const ascending = s.sortingColumn ? s.sortAscending : true;
  const shown = rows.filter((r) => netShown(r, s));
  const rules = s.customGroupRules.map((r) => new EdaCombinedMatcher(r, "net"));
  const custom = new Map<string, NetInspectorRow[]>(s.customGroupRules.map((r) => [r, []]));
  const byClass = new Map<string, NetInspectorRow[]>();
  const loose: NetInspectorRow[] = [];
  for (const r of shown) {
    // "First see if item matches a group-by rule": `rule->Find( GetNetName() )`, the unescaped name
    const rule = rules.find((m) => m.find(unescapeString(r.name)));
    if (rule) custom.get(rule.pattern)!.push(r);
    else if (s.groupByNetclass) {
      const list = byClass.get(r.netclass) ?? [];
      list.push(r);
      byClass.set(r.netclass, list);
    } else loose.push(r);
  }
  const order = new Map(rows.map((r, i) => [r, i]));
  const cmpRows = (a: NetInspectorRow, b: NetInspectorRow): number => {
    const c = compareValues(rowValue(a, column, s.showTimeDomainDetails), rowValue(b, column, s.showTimeDomainDetails));
    // "when the item values compare equal resort to pointer comparison": the order the rows came in
    return c !== 0 ? (ascending ? c : -c) : (order.get(a) ?? 0) - (order.get(b) ?? 0);
  };
  const groups: NetInspectorGroup[] = [];
  for (const [name, children] of custom) groups.push({ type: "custom", name, children: [...children].sort(cmpRows), totals: sumRows(name, children) });
  for (const [name, children] of byClass) groups.push({ type: "netclass", name, children: [...children].sort(cmpRows), totals: sumRows(name, children) });
  const groupOrder = new Map(groups.map((g, i) => [g, i]));
  groups.sort((a, b) => {
    // a group's Name is its group name, its Netclass empty (`GetNetclassName()`)
    const c = column === "netclass" ? 0 : compareValues(rowValue(a.totals, column, s.showTimeDomainDetails), rowValue(b.totals, column, s.showTimeDomainDetails));
    return c !== 0 ? (ascending ? c : -c) : groupOrder.get(a)! - groupOrder.get(b)!;
  });
  const out: NetInspectorLine[] = [];
  for (const g of groups) {
    const expanded = s.expandedRows.includes(g.name);
    out.push({ kind: "group", group: g, expanded });
    if (expanded) for (const r of g.children) out.push({ kind: "net", row: r, depth: 1, group: g });
  }
  for (const r of [...loose].sort(cmpRows)) out.push({ kind: "net", row: r, depth: 0, group: null });
  return out;
}

// ----------------------------------------------------------------------------------------------------------------------------------------- the cells

/** `formatDelay`: `MessageTextFromValue( aValue, .., EDA_DATA_TYPE::TIME )` -- picoseconds (`IU_PER_PS` is 1e6 attoseconds), `%.2f`, " ps". */
export function formatDelay(attoseconds: number, addUnitLabel = true): string {
  const ps = attoseconds / 1_000_000;
  let text = ps.toFixed(2);
  // "Check if the formatted value shows only zeros but the actual value is non-zero": `%.3e`
  if (ps !== 0 && !/[1-9]/.test(text)) {
    const [m, e] = ps.toExponential(3).split("e");
    text = `${m}e${e!.startsWith("-") ? "-" : "+"}${e!.replace(/^[+-]/, "").padStart(2, "0")}`;
  }
  return addUnitLabel ? `${text} ps` : text;
}

/** Whether a column holds a length (or, in the time domain, a delay). */
function hasUnits(column: NetInspectorColumn): boolean {
  return isLayerColumn(column) || (NET_INSPECTOR_COLUMNS.find((c) => c.key === column)?.units ?? false);
}

/** A cell of a net row as the panel shows it (`formatLength`, `formatDelay`, `formatCount`), or as the report writes it (no unit label). */
export function netInspectorCell(row: NetInspectorRow, column: NetInspectorColumn, units: MessageUnits, report = false, timeDomain = false): string {
  const v = rowValue(row, column, timeDomain);
  if (column === "name") return unescapeString(String(v));
  if (hasUnits(column)) return timeDomain ? formatDelay(v as number, !report) : messageTextFromValue(v as number, units, !report);
  return String(v);
}

/** A cell of a group row (`DATA_MODEL::GetValue` of a group): "Netclass: <name>" or "Custom: <pattern>", an empty class, the sums. */
export function netInspectorGroupCell(group: NetInspectorGroup, column: NetInspectorColumn, units: MessageUnits, timeDomain = false): string {
  if (column === "name") return `${group.type === "netclass" ? "Netclass" : "Custom"}: ${group.name}`;
  if (column === "netclass") return "";
  return netInspectorCell(group.totals, column, units, false, timeDomain);
}

/** `EDA_UNIT_UTILS::GetLabel`. */
function unitLabel(units: MessageUnits): string {
  return units === "mm" ? "mm" : units === "mil" ? "mils" : "in";
}

/**
 * `generateReport`: the file's text. Every column of `m_columns` (a hidden one too), the header `"<csv name> (<units>)"` for one with units -- the user's length
 * units even for a delay column, as KiCad writes them -- and a line per listed net (`if( i.GetIsGroup() || i.GetNetCode() == 0 ) continue;`).
 */
export function netInspectorReport(rows: readonly NetInspectorRow[], units: MessageUnits, columns: readonly NetInspectorColumnDesc[] = NET_INSPECTOR_COLUMNS, timeDomain = false): string {
  const lines: string[] = [];
  lines.push(columns.map((c) => `"${c.units ? `${c.csv} (${unitLabel(units)})` : c.csv}";`).join(""));
  for (const r of rows) {
    if (r.name === "") continue;
    lines.push(columns.map((c) => (c.key === "name" || c.key === "netclass" ? `"${netInspectorCell(r, c.key, units, true, timeDomain)}";` : `${netInspectorCell(r, c.key, units, true, timeDomain)};`)).join(""));
  }
  return lines.join("\n") + "\n";
}

// ----------------------------------------------------------------------------------------------------------------------------------------- the menus

export type NetInspectorMenuId =
  | "filter_by_net_name"
  | "filter_by_netclass"
  | "group_by_netclass"
  | "add_group"
  | "remove_selected_group"
  | "remove_groups"
  | "show_zero_pad_nets"
  | "show_unconnected_nets"
  | "show_time_domain_details"
  | "generate_report"
  | "highlight_selected_nets"
  | "clear_highlighting"
  | "rename_net"
  | "delete_net"
  | "add_net"
  | `hide_column:${NetInspectorColumn}`;

export type NetInspectorMenuEntry = { kind: "separator" } | { kind: "item"; id: NetInspectorMenuId; label: string; enabled: boolean; checked?: boolean } | { kind: "submenu"; label: string; items: NetInspectorMenuEntry[] };

const item = (id: NetInspectorMenuId, label: string, enabled = true, checked?: boolean): NetInspectorMenuEntry => (checked === undefined ? { kind: "item", id, label, enabled } : { kind: "item", id, label, enabled, checked });
const SEP: NetInspectorMenuEntry = { kind: "separator" };

/** What is selected in the list, as the menus ask it. */
export interface NetInspectorSelectionFacts {
  /** `GetSelectedItemsCount()`. */
  count: number;
  /** The one selected row is a group (`selItem->GetIsGroup()`). */
  oneIsGroup: boolean;
}

/** `generateShowHideColumnMenu`: every column but the Name, the fixed ones, a separator, the layers'; checked when shown. */
export function showHideColumnsMenu(columns: readonly NetInspectorColumnDesc[], hidden: readonly NetInspectorColumn[]): NetInspectorMenuEntry[] {
  const entry = (c: NetInspectorColumnDesc) => item(`hide_column:${c.key}`, c.label, true, !hidden.includes(c.key));
  return [...columns.filter((c) => c.key !== "name" && !isLayerColumn(c.key)).map(entry), SEP, ...columns.filter((c) => isLayerColumn(c.key)).map(entry)];
}

/** `OnConfigButton`: the configure button's menu. */
export function netInspectorConfigMenu(s: NetInspectorSettings, sel: NetInspectorSelectionFacts, columns: readonly NetInspectorColumnDesc[]): NetInspectorMenuEntry[] {
  return [
    item("filter_by_net_name", "Filter by Net Name", true, s.filterByNetName),
    item("filter_by_netclass", "Filter by Netclass", true, s.filterByNetclass),
    SEP,
    item("group_by_netclass", "Group by Netclass", true, s.groupByNetclass),
    SEP,
    item("add_group", "Add Custom Group..."),
    item("remove_selected_group", "Remove Selected Custom Group", sel.count === 1 && sel.oneIsGroup),
    item("remove_groups", "Remove All Custom Groups", s.customGroupRules.length !== 0),
    SEP,
    item("show_zero_pad_nets", "Show Zero Pad Nets", true, s.showZeroPadNets),
    item("show_unconnected_nets", "Show Unconnected Nets", true, s.showUnconnectedNets),
    SEP,
    item("show_time_domain_details", "Show Time Domain Details", true, s.showTimeDomainDetails),
    SEP,
    item("generate_report", "Save Net Inspector Report..."),
    SEP,
    { kind: "submenu", label: "Show / Hide Columns", items: showHideColumnsMenu(columns, s.hiddenColumns) },
  ];
}

/** `OnNetsListContextMenu`: the rows' menu. `anyHighlight` is whether the board has a net highlighted (`GetHighlightNetCodes().size()`). */
export function netInspectorContextMenu(sel: NetInspectorSelectionFacts, anyHighlight: boolean): NetInspectorMenuEntry[] {
  const none = sel.count === 0;
  const many = sel.count > 1 || (sel.count === 1 && sel.oneIsGroup);
  return [
    item("highlight_selected_nets", many ? "Highlight Selected Nets" : "Highlight Selected Net", !none),
    item("clear_highlighting", "Clear Net Highlighting", anyHighlight),
    SEP,
    item("rename_net", "Rename Selected Net...", !none && !many),
    item("delete_net", many ? "Delete Selected Nets" : "Delete Selected Net", !none),
    SEP,
    item("add_net", "Add Net..."),
    SEP,
    item("remove_selected_group", "Remove Selected Custom Group", sel.count === 1 && sel.oneIsGroup),
  ];
}

/** `onContextMenuSelection`'s toggles and `ID_REMOVE_GROUPS`: the settings after a checked entry (null for an entry that is no toggle). */
export function toggleSetting(s: NetInspectorSettings, id: NetInspectorMenuId): NetInspectorSettings | null {
  switch (id) {
    case "filter_by_net_name":
      return { ...s, filterByNetName: !s.filterByNetName };
    case "filter_by_netclass":
      return { ...s, filterByNetclass: !s.filterByNetclass };
    case "group_by_netclass":
      return { ...s, groupByNetclass: !s.groupByNetclass };
    case "show_zero_pad_nets":
      return { ...s, showZeroPadNets: !s.showZeroPadNets };
    case "show_unconnected_nets":
      return { ...s, showUnconnectedNets: !s.showUnconnectedNets };
    case "show_time_domain_details":
      return { ...s, showTimeDomainDetails: !s.showTimeDomainDetails };
    case "remove_groups":
      return { ...s, customGroupRules: [] };
  }
  if (id.startsWith("hide_column:")) {
    const c = id.slice("hide_column:".length) as NetInspectorColumn;
    return { ...s, hiddenColumns: s.hiddenColumns.includes(c) ? s.hiddenColumns.filter((x) => x !== c) : [...s.hiddenColumns, c] };
  }
  return null;
}

/** A click on a column heading (`OnColumnSorted`): the same column again turns the order round, another sorts ascending by it. */
export function sortBy(s: NetInspectorSettings, column: NetInspectorColumn): NetInspectorSettings {
  const current = s.sortingColumn ?? "name";
  const ascending = s.sortingColumn ? s.sortAscending : true;
  return { ...s, sortingColumn: column, sortAscending: current === column ? !ascending : true };
}

/** `OnExpandCollapseRow`: a group opened or closed. */
export function toggleExpanded(s: NetInspectorSettings, groupName: string): NetInspectorSettings {
  return { ...s, expandedRows: s.expandedRows.includes(groupName) ? s.expandedRows.filter((g) => g !== groupName) : [...s.expandedRows, groupName] };
}

// ----------------------------------------------------------------------------------------------------------------------------------------- net edits

/** `NETNAME_VALIDATOR::IsValid` (spaces allowed): the message, or "" for a valid name. Shown under the title "Invalid signal name". */
export function netNameInvalid(name: string): string {
  if (name.includes("\r") || name.includes("\n")) return "Signal names cannot contain CR or LF characters";
  return "";
}

/** `onAddGroup`: the custom group rules after the pattern typed (`UnescapeString`); a pattern already a rule is not added twice; empty adds nothing. */
export function addGroupRule(s: NetInspectorSettings, typed: string): NetInspectorSettings {
  const name = unescapeString(typed);
  if (name === "" || s.customGroupRules.includes(name)) return s;
  return { ...s, customGroupRules: [...s.customGroupRules, name] };
}

/** `onRemoveSelectedGroup`: the rule of the selected group goes (a netclass group has none: nothing changes). */
export function removeGroupRule(s: NetInspectorSettings, groupName: string): NetInspectorSettings {
  return s.customGroupRules.includes(groupName) ? { ...s, customGroupRules: s.customGroupRules.filter((r) => r !== groupName) } : s;
}

/** `onRenameSelectedNet`: the net's path (up to its last `/`, kept) and its short name, unescaped -- what the "Rename Net" dialog starts with. */
export function renameParts(fullName: string): { path: string; short: string } {
  const at = fullName.lastIndexOf("/");
  return at >= 0 ? { path: fullName.slice(0, at + 1), short: unescapeString(fullName.slice(at + 1)) } : { path: "", short: unescapeString(fullName) };
}

/**
 * One OK of the "Rename Net" dialog (`onRenameSelectedNet`'s loop): `done` when the name typed is `unchanged` (the short name the first time; after an error
 * KiCad's `unescapedShortName` is "", so an empty name then closes the dialog), an `error` to show before the dialog comes again holding `retry`, or the new full
 * name (`netPath + EscapeString( short, CTX_NETNAME )`). `inUse` is `BOARD::FindNet`, asked of the short name and of the full one.
 */
export type RenameStep = { kind: "done" } | { kind: "error"; message: string; retry: string } | { kind: "rename"; to: string };

export function renameStep(fullName: string, typed: string, unchanged: string, inUse: (name: string) => boolean): RenameStep {
  const { path } = renameParts(fullName);
  if (typed === unchanged) return { kind: "done" };
  if (typed === "") return { kind: "error", message: "Net name cannot be empty.", retry: "" };
  const escaped = escapeNetname(typed);
  const full = path + escaped;
  if (inUse(escaped) || inUse(full)) return { kind: "error", message: `Net name '${typed}' is already in use.`, retry: "" };
  return { kind: "rename", to: full };
}

/** One OK of the "New Net" dialog (`onAddNet`): nothing for an empty name, an error for one in use (the dialog again, empty), else the name as typed. */
export function addNetStep(typed: string, inUse: (name: string) => boolean): RenameStep {
  if (typed === "") return { kind: "done" };
  if (inUse(typed)) return { kind: "error", message: `Net name '${typed}' is already in use.`, retry: "" };
  return { kind: "rename", to: typed };
}

/** The question `onDeleteSelectedNet` asks before deleting a net that has pads ("Net '%s' is in use.  Delete anyway?"), or null for one with none. */
export function deleteNetQuestion(row: NetInspectorRow): string | null {
  return row.pad_count === 0 ? null : `Net '${unescapeString(row.name)}' is in use.  Delete anyway?`;
}

/** The question before deleting a group's nets ("Delete all nets in group '%s'?"), or null for an empty group (nothing is deleted then). */
export function deleteGroupQuestion(group: NetInspectorGroup): string | null {
  return group.children.length === 0 ? null : `Delete all nets in group '${group.name}'?`;
}
