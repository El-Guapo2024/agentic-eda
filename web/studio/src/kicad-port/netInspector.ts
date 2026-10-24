// The Net Inspector (`PCB_NET_INSPECTOR_PANEL`, pcbnew/widgets/pcb_net_inspector_panel.cpp at 8303b2ad): its columns, its filter and its report.
//
// The numbers are the backend's (`GET /api/net_inspector`, crates/cli/src/net_inspector_api.rs: `calculateNets` and
// `LENGTH_DELAY_CALCULATION::CalculateLengthDetails`); this file is what the panel does with them:
//   * the columns (`buildColumns`): Name, Netclass, Total Length, Via Count, Via Length, Track Length, Die Length, Pad Count, the lengths in the
//     user's units by `MessageTextFromValue` (`formatLength`);
//   * which nets are listed (`netFilterMatches`): never the unconnected net; the search text matched, upper-cased, as a substring of the net's
//     name ("Filter by Net Name") or its class's ("Filter by Netclass"); a net named `unconnected-(...` only with "Show Unconnected Nets"; a
//     net with no pad only with "Show Zero Pad Nets";
//   * the sort of a column (the data model's `compareItems`: by the value, the name breaking a tie);
//   * Save Net Inspector Report... (`generateReport`): the CSV KiCad writes, `;` after every field, the text columns quoted, the lengths without
//     their unit label and the unit in the header.
//
// Pure: no store, no DOM. Unit-tested in netInspector.test.ts.
import { messageTextFromValue, type MessageUnits } from "./messageText";

/** One net's row, as the backend sends it (lengths in nm). */
export interface NetInspectorRow {
  name: string;
  netclass: string;
  total_length: number;
  via_count: number;
  via_length: number;
  track_length: number;
  die_length: number;
  pad_count: number;
  /** Track length per copper layer (`LENGTH_DELAY_STATS::LayerLengths`), nm; a layer the net has no track on is absent. */
  layer_lengths?: Record<string, number>;
}

/** A fixed column, or `layer:<name>` for the length on one copper layer. */
export type NetInspectorColumn = Exclude<keyof NetInspectorRow, "layer_lengths"> | `layer:${string}`;

export interface NetInspectorColumnDesc {
  key: NetInspectorColumn;
  label: string;
  csv: string;
  units: boolean;
}

/** `buildColumns`: (key, display name, CSV name, has units). */
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

/**
 * Every column of the panel: the fixed ones, then one per enabled copper layer (`for( PCB_LAYER_ID layer : m_board->GetEnabledLayers().Seq() )`, in
 * layer-id order: F.Cu, B.Cu, then the inner layers), named by the layer, a length in the user's units.
 */
export function netInspectorColumns(copperLayers: readonly string[]): NetInspectorColumnDesc[] {
  const order = (l: string) => (l === "F.Cu" ? -2 : l === "B.Cu" ? -1 : Number(/^In(\d+)\.Cu$/.exec(l)?.[1] ?? 1000));
  const layers = [...copperLayers].sort((a, b) => order(a) - order(b));
  return [...NET_INSPECTOR_COLUMNS, ...layers.map((l): NetInspectorColumnDesc => ({ key: `layer:${l}`, label: l, csv: l, units: true }))];
}

/** The value of a column of a row (a layer column is the row's length on that layer, 0 for none). */
function valueOf(row: NetInspectorRow, column: NetInspectorColumn): string | number {
  if (column.startsWith("layer:")) return row.layer_lengths?.[column.slice(6)] ?? 0;
  return row[column as Exclude<keyof NetInspectorRow, "layer_lengths">] as string | number;
}

/** `PANEL_NET_INSPECTOR_SETTINGS`, with the project's defaults (`project_local_settings.cpp`). */
export interface NetInspectorOptions {
  filter: string;
  filterByNetName: boolean;
  filterByNetclass: boolean;
  showZeroPadNets: boolean;
  showUnconnectedNets: boolean;
}

export const DEFAULT_NET_INSPECTOR_OPTIONS: NetInspectorOptions = { filter: "", filterByNetName: true, filterByNetclass: true, showZeroPadNets: false, showUnconnectedNets: false };

/** `netFilterMatches` and `calculateNets`'s zero-pad test. */
export function netShown(row: NetInspectorRow, o: NetInspectorOptions): boolean {
  if (row.name === "") return false; // "Never show an unconnected net"
  if (!o.showZeroPadNets && row.pad_count === 0) return false;
  const filter = o.filter.toUpperCase();
  const name = row.name.toUpperCase();
  let matched = filter.length === 0;
  if (!matched && o.filterByNetclass && row.netclass.toUpperCase().includes(filter)) matched = true;
  if (!matched && o.filterByNetName && name.includes(filter)) matched = true;
  if (matched && !o.showUnconnectedNets) matched = !name.startsWith("UNCONNECTED-(");
  return matched;
}

/** The rows shown, sorted by `column` (the name breaking a tie). */
export function netInspectorRows(rows: readonly NetInspectorRow[], o: NetInspectorOptions, column: NetInspectorColumn = "name", ascending = true): NetInspectorRow[] {
  const shown = rows.filter((r) => netShown(r, o));
  const cmp = (a: NetInspectorRow, b: NetInspectorRow): number => {
    const x = valueOf(a, column);
    const y = valueOf(b, column);
    const c = typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y));
    return c !== 0 ? c : a.name.localeCompare(b.name);
  };
  shown.sort((a, b) => (ascending ? cmp(a, b) : cmp(b, a)));
  return shown;
}

/** A cell as the panel shows it (`formatLength`, `formatCount`), or as the report writes it (no unit label). */
export function netInspectorCell(row: NetInspectorRow, column: NetInspectorColumn, units: MessageUnits, report = false): string {
  const v = valueOf(row, column);
  const lengthColumn = column.startsWith("layer:") || (NET_INSPECTOR_COLUMNS.find((c) => c.key === column)?.units ?? false);
  if (lengthColumn) return messageTextFromValue(v as number, units, !report);
  return String(v);
}

/** `EDA_UNIT_UTILS::GetLabel`. */
function unitLabel(units: MessageUnits): string {
  return units === "mm" ? "mm" : units === "mil" ? "mils" : "in";
}

/** `generateReport`: the file's text. */
export function netInspectorReport(rows: readonly NetInspectorRow[], units: MessageUnits, columns: readonly NetInspectorColumnDesc[] = NET_INSPECTOR_COLUMNS): string {
  const lines: string[] = [];
  lines.push(columns.map((c) => `"${c.units ? `${c.csv} (${unitLabel(units)})` : c.csv}";`).join(""));
  for (const r of rows) {
    lines.push(columns.map((c) => (c.key === "name" || c.key === "netclass" ? `"${netInspectorCell(r, c.key, units, true)}";` : `${netInspectorCell(r, c.key, units, true)};`)).join(""));
  }
  return lines.join("\n") + "\n";
}
