// KiCad 10's Drill Chart Properties and Drill Groups dialogs (KiCad 25ec2335; the 8303b2ad source the rest of the port reads predates them): what they show and what OK sends.
//
//   pcbnew/dialogs/dialog_generated_table_properties.cpp   DIALOG_GENERATED_TABLE_PROPERTIES: setColumns, fillColumnGrid, harvestColumnGrid, shownColumns,
//                                                          moveColumn, onResetColumns, onBorderChecked, TransferDataFromWindow     [`columnRows`], [`moveColumnRow`],
//                                                                                                                                  [`drillChartEditCmd`]
//   pcbnew/dialogs/panel_drill_chart_options.cpp           PANEL_DRILL_CHART_OPTIONS: the filter, the totals row, the grouping checklist  [`GROUP_KEY_ROWS`]
//   pcbnew/drill/drill_chart_template.cpp                  DrillChartSchema, DRILL_CHART_TEMPLATE::SaveToFile, LoadFromFile           [`COLUMN_DEFS`], [`templateJson`],
//                                                                                                                                  [`readTemplate`]
//   pcbnew/board_tables/generated_table_schema.cpp         GENERATED_TABLE_SCHEMA::Validate, Defaults, DefaultColumn                  [`validateColumns`], [`defaultColumns`]
//   pcbnew/dialogs/dialog_drill_groups.cpp                 DIALOG_DRILL_GROUPS: markText, Reload's cells, commitDetail                [`groupCells`], [`markEditCmd`]
//   pcbnew/tools/pcb_selection_tool.cpp                    PCB_SELECTION_TOOL::selectChartRow (a double click on a data row)          [`chartRowCells`]
//
// Pure: no React, no store. Unit-tested in drillChartDialog.test.ts.
import type { BoardDrillChart, DrillChartFilter, TableUnits } from "../api/pcbItemTypes";
import type { BoardTable, Cmd } from "../api/types";
import { cellHitDistance } from "./tables";

export type ColumnAlign = "left" | "center" | "right";

/** `GENERATED_TABLE_COLUMN`: a chart's column -- its id (the schema's token), heading, alignment and minimum width (0: as wide as its text). */
export interface ChartColumn {
  id: string;
  heading: string;
  align: ColumnAlign;
  width_nm: number;
}

/** `GENERATED_TABLE_COLUMN_DEF` of `DrillChartSchema()`: the token, the heading a new column gets, its alignment, whether `Defaults()` has it, and the name the Columns page shows. */
export interface ColumnDef {
  token: string;
  heading: string;
  align: ColumnAlign;
  defaultShown: boolean;
  label: string;
}

/** `DrillChartSchema()`, in its order (the order the Columns page lists the columns a chart does not show). */
export const COLUMN_DEFS: readonly ColumnDef[] = [
  { token: "symbol", heading: "SYM", align: "center", defaultShown: true, label: "Symbol" },
  { token: "drill_diameter", heading: "DRILL DIA", align: "right", defaultShown: true, label: "Drill diameter" },
  { token: "slot_size", heading: "SLOT W x L", align: "right", defaultShown: true, label: "Slot size" },
  { token: "plating", heading: "PLATED", align: "center", defaultShown: true, label: "Plating" },
  { token: "operation_count", heading: "OPS", align: "right", defaultShown: true, label: "Operation count" },
  { token: "site_count", heading: "SITE COUNT", align: "right", defaultShown: false, label: "Site count" },
  { token: "layer_span", heading: "FROM / TO", align: "center", defaultShown: true, label: "Layer span" },
  { token: "operation", heading: "OPERATION", align: "left", defaultShown: false, label: "Operation" },
  { token: "protection", heading: "PROTECTION", align: "left", defaultShown: false, label: "Protection" },
  { token: "backdrill_stub", heading: "BACKDRILL STUB", align: "right", defaultShown: false, label: "Backdrill stub" },
  { token: "aspect_ratio", heading: "ASPECT RATIO", align: "right", defaultShown: false, label: "Aspect ratio" },
  { token: "description", heading: "DESCRIPTION", align: "left", defaultShown: false, label: "Description" },
];

/** `alignChoices()`: the Align column's choices, in `GENERATED_TABLE_ALIGN` order. */
export const ALIGN_CHOICES: readonly [ColumnAlign, string][] = [
  ["left", "Left"],
  ["center", "Center"],
  ["right", "Right"],
];

/** `m_unitsCtrlChoices`, in `GENERATED_TABLE_UNITS` order. */
export const UNITS_CHOICES: readonly [TableUnits, string][] = [
  ["mm", "mm"],
  ["inch", "inch"],
  ["mils", "mils"],
];

/** `g_groupKeyRows` with `m_groupByListChoices`: the "Group holes by" checklist, row by row ("Explicit rather than positional"). */
export const GROUP_KEY_ROWS: readonly [string, string][] = [
  ["size", "Hole size"],
  ["slot", "Slot shape"],
  ["plating", "Plating"],
  ["span", "Layer span"],
  ["operation", "Drill operation"],
  ["hole_function", "Hole function"],
  ["protection", "IPC-4761 protection"],
  ["post_machining", "Post-machining"],
];

/** `m_markPolicyCtrlChoices` (Board Setup's Drill Chart page), in `DRILL_MARK_POLICY` order. */
export const MARK_POLICY_CHOICES: readonly [string, string][] = [
  ["shapes", "Curated shapes"],
  ["letters", "Letters"],
  ["shapes_then_letters", "Curated shapes, then letters"],
  ["size_text", "Hole size text"],
];

/** `m_markCtrlChoices` (Drill Groups' Mark), in `DRILL_MARK_MODE` order. */
export const MARK_MODE_CHOICES: readonly [MarkMode, string][] = [
  ["shape", "Shape"],
  ["letter", "Letter"],
  ["size_text", "Hole size text"],
];

export type MarkMode = "shape" | "letter" | "size_text";

/** `DRILL_CHART_FILTER()`: every kind of hole. */
export const ALL_HOLES: DrillChartFilter = { plated: true, non_plated: true, vias: true, slots: true, backdrills: true, castellated: true };

/** `GENERATED_TABLE_MAX_COLUMN_WIDTH`: "No real table column is a metre across." */
export const MAX_COLUMN_WIDTH_NM = 1_000_000_000;
/** `GENERATED_TABLE_MAX_TOTAL_WIDTH`. */
export const MAX_TOTAL_WIDTH_NM = 10_000_000_000;

export function columnDef(token: string): ColumnDef | undefined {
  return COLUMN_DEFS.find((d) => d.token === token);
}

/** `columnLabel`: what the column reports, "whatever the user has retitled its heading to". */
export function columnLabel(token: string): string {
  const d = columnDef(token);
  return d ? d.label || d.heading : "";
}

/** `GENERATED_TABLE_SCHEMA::DefaultColumn`. */
export function defaultColumn(token: string): ChartColumn {
  const d = columnDef(token);
  return { id: token, heading: d?.heading ?? "", align: d?.align ?? "left", width_nm: 0 };
}

/** `GENERATED_TABLE_SCHEMA::Defaults`: the columns a new chart has (and Reset to Default restores). */
export function defaultColumns(): ChartColumn[] {
  return COLUMN_DEFS.filter((d) => d.defaultShown).map((d) => defaultColumn(d.token));
}

/** A row of the Columns page (`COLUMN_ROW`): the column and whether the chart shows it. */
export interface ColumnRow {
  column: ChartColumn;
  shown: boolean;
}

/** `setColumns`: the chart's columns, shown, in their order, then every column of the schema it does not show, as `DefaultColumn` makes it, hidden. */
export function columnRows(columns: readonly ChartColumn[]): ColumnRow[] {
  const rows: ColumnRow[] = columns.map((c) => ({ column: { ...c }, shown: true }));
  for (const d of COLUMN_DEFS) if (!columns.some((c) => c.id === d.token)) rows.push({ column: defaultColumn(d.token), shown: false });
  return rows;
}

/** `moveColumn( delta )`: the row at `row` swapped with its neighbour; null when there is none (the grid cursor stays). */
export function moveColumnRow(rows: readonly ColumnRow[], row: number, delta: -1 | 1): { rows: ColumnRow[]; row: number } | null {
  const target = row + delta;
  if (row < 0 || target < 0 || target >= rows.length) return null;
  const next = [...rows];
  [next[row], next[target]] = [next[target]!, next[row]!];
  return { rows: next, row: target };
}

/** `shownColumns`: the columns of the rows that are shown, in the page's order, each width clamped to `[0, GENERATED_TABLE_MAX_COLUMN_WIDTH]` (`harvestColumnGrid`). */
export function shownColumns(rows: readonly ColumnRow[]): ChartColumn[] {
  return rows.filter((r) => r.shown).map((r) => ({ ...r.column, width_nm: Math.min(Math.max(0, Math.round(r.column.width_nm || 0)), MAX_COLUMN_WIDTH_NM) }));
}

/** `GENERATED_TABLE_SCHEMA::Validate`: an unknown or repeated column dropped, the widths clamped; false when nothing is left or the total is past ten metres. */
export function validateColumns(columns: readonly ChartColumn[]): { columns: ChartColumn[]; ok: boolean } {
  const seen = new Set<string>();
  const kept: ChartColumn[] = [];
  let total = 0;
  for (const c of columns) {
    if (!columnDef(c.id) || seen.has(c.id)) continue;
    seen.add(c.id);
    const width_nm = Math.min(Math.max(0, Math.round(c.width_nm || 0)), MAX_COLUMN_WIDTH_NM);
    total += width_nm;
    kept.push({ ...c, width_nm });
  }
  return { columns: kept, ok: kept.length > 0 && total <= MAX_TOTAL_WIDTH_NM };
}

/** What the dialog's three pages hold when OK is pressed. */
export interface DrillChartForm {
  layer: string;
  locked: boolean;
  units: TableUnits;
  precision: number;
  rows: ColumnRow[];
  filter: DrillChartFilter;
  showTotals: boolean;
  groupBy: string[];
  externalBorder: boolean;
  headerSeparator: boolean;
  rowSeparators: boolean;
  columnSeparators: boolean;
  /** "Border width:" / "Separator width:", nm; negative is "no stroke" (the field is empty). */
  borderWidthNm: number;
  separatorsWidthNm: number;
}

/** `TransferDataToWindow`: the form a chart opens with (the grouping is the board's profile's). */
export function drillChartForm(chart: BoardDrillChart, table: BoardTable, locked: boolean, groupBy: readonly string[]): DrillChartForm {
  return {
    layer: table.layer,
    locked,
    units: chart.units ?? "mm",
    precision: chart.precision ?? 3,
    rows: columnRows(chart.columns.map((c) => ({ id: c.id, heading: c.heading, align: c.align, width_nm: c.width_nm ?? 0 }))),
    filter: { ...ALL_HOLES, ...(chart.filter ?? {}) },
    showTotals: chart.show_totals ?? true,
    groupBy: [...groupBy],
    externalBorder: table.border.external,
    headerSeparator: table.border.header,
    rowSeparators: table.separators.rows,
    columnSeparators: table.separators.cols,
    borderWidthNm: table.border.stroke.width,
    separatorsWidthNm: table.separators.stroke.width,
  };
}

/** `onBorderChecked`: a width switched on starts at the layer's line thickness when it had none; the widths are enabled while their lines are drawn. */
export function onBorderChecked(f: DrillChartForm, defaultWidthNm: number): DrillChartForm {
  const border = f.externalBorder || f.headerSeparator;
  const separators = f.rowSeparators || f.columnSeparators;
  return { ...f, borderWidthNm: border && f.borderWidthNm < 0 ? defaultWidthNm : f.borderWidthNm, separatorsWidthNm: separators && f.separatorsWidthNm < 0 ? defaultWidthNm : f.separatorsWidthNm };
}

/**
 * `TransferDataFromWindow`: the error KiCad shows ("A table needs at least one column.", "The columns are too wide in total."), or the commands of "Edit Drill Chart" -- the
 * chart's `edit_drill_chart` and, when the lock changed, `set_locked`, one undo step.
 */
export function drillChartEditCmd(chartId: string, f: DrillChartForm, wasLocked: boolean): { error: string } | { cmds: Cmd[] } {
  const columns = shownColumns(f.rows);
  if (columns.length === 0) return { error: "A table needs at least one column." };
  if (!validateColumns(columns).ok) return { error: "The columns are too wide in total." };
  const cmds: Cmd[] = [
    {
      op: "pcb_item",
      verb: "edit_drill_chart",
      id: chartId,
      chart: {
        layer: f.layer,
        units: f.units,
        precision: Math.min(6, Math.max(0, Math.trunc(f.precision))),
        columns,
        filter: f.filter,
        show_totals: f.showTotals,
        group_by: GROUP_KEY_ROWS.map(([k]) => k).filter((k) => f.groupBy.includes(k)),
        external_border: f.externalBorder,
        header_separator: f.headerSeparator,
        row_separators: f.rowSeparators,
        column_separators: f.columnSeparators,
        border_width_nm: Math.round(f.borderWidthNm),
        separators_width_nm: Math.round(f.separatorsWidthNm),
      },
    },
  ];
  if (f.locked !== wasLocked) cmds.push({ op: "set_locked", ids: [chartId], locked: f.locked });
  return { cmds };
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- templates

/** `DRILL_CHART_TEMPLATE`. */
export interface ChartTemplate {
  name: string;
  version: number;
  units: TableUnits;
  precision: number;
  showTotals: boolean;
  columns: ChartColumn[];
}

/** `nlohmann::json::dump( 2 )` of an object: its keys in their byte order (the json's `std::map`), two spaces a level. */
function sortedJson(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(sortedJson);
  if (v && typeof v === "object") return Object.fromEntries(Object.keys(v as object).sort().map((k) => [k, sortedJson((v as Record<string, unknown>)[k])]));
  return v;
}

/** `DRILL_CHART_TEMPLATE::SaveToFile`: the file's text (`js.dump( 2 )` and `std::endl`). The name is the file's ("The file names the template"). */
export function templateJson(t: ChartTemplate): string {
  const js = {
    name: t.name,
    version: t.version,
    units: t.units,
    precision: t.precision,
    show_totals: t.showTotals,
    columns: t.columns.map((c) => ({ id: c.id, heading: c.heading, align: c.align, width: Math.round(c.width_nm) })),
  };
  return JSON.stringify(sortedJson(js), null, 2) + "\n";
}

/** `DRILL_CHART_TEMPLATE::LoadFromFile`: the template, or the message KiCad shows ("Template is not a JSON object.", the parser's own, "Template has no usable columns."). */
export function readTemplate(text: string): ChartTemplate | { error: string } {
  let js: unknown;
  try {
    js = JSON.parse(text);
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
  if (!js || typeof js !== "object" || Array.isArray(js)) return { error: "Template is not a JSON object." };
  const o = js as Record<string, unknown>;
  const isInt = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v);
  const t: ChartTemplate = { name: "", version: 1, units: "mm", precision: 3, showTotals: true, columns: [] };
  if (typeof o.name === "string") t.name = o.name;
  if (isInt(o.version)) t.version = o.version;
  // `GeneratedTableUnitsFromToken`: an unknown token leaves the units as they were.
  if (typeof o.units === "string" && UNITS_CHOICES.some(([u]) => u === o.units)) t.units = o.units as TableUnits;
  if (isInt(o.precision)) t.precision = Math.min(6, Math.max(0, o.precision));
  if (typeof o.show_totals === "boolean") t.showTotals = o.show_totals;
  if (Array.isArray(o.columns)) {
    for (const entry of o.columns) {
      if (!entry || typeof entry !== "object" || Array.isArray(entry) || typeof (entry as Record<string, unknown>).id !== "string") continue;
      const e = entry as Record<string, unknown>;
      // "Skipped rather than defaulted, so a template written by a newer KiCad does not silently turn an unknown column into the symbol column"
      const def = columnDef(e.id as string);
      if (!def) continue;
      const col: ChartColumn = { id: def.token, heading: "", align: "left", width_nm: 0 };
      if (typeof e.heading === "string") col.heading = e.heading;
      if (typeof e.align === "string" && ALIGN_CHOICES.some(([a]) => a === e.align)) col.align = e.align as ColumnAlign;
      if (isInt(e.width)) col.width_nm = e.width;
      t.columns.push(col);
    }
  }
  const v = validateColumns(t.columns);
  if (!v.ok) return { error: "Template has no usable columns." };
  return { ...t, columns: v.columns };
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- Drill Groups

/** One group as `GET /api/drill_groups` sends it (`DRILL_CHART_GROUP` with the resolver's mark). */
export interface DrillGroupRow {
  key: string;
  symbol: { mode: MarkMode; shape_index?: number; letter?: string; description?: string };
  mark_text: string;
  diameter_nm: number;
  is_slot: boolean;
  size_nm: [number, number];
  not_plated: boolean;
  top: string;
  bottom: string;
  operations: number;
  sites: number;
  members: string[];
}

/** `Reload`'s cells of a group's row: Symbol, Size, Plated, From / To, Ops, Sites. */
export function groupCells(g: DrillGroupRow): [string, string, string, string, string, string] {
  const mm3 = (nm: number) => (nm / 1e6).toFixed(3);
  const size = g.is_slot ? `${mm3(g.size_nm[0])} x ${mm3(g.size_nm[1])}` : mm3(g.diameter_nm);
  return [g.mark_text, size, g.not_plated ? "No" : "Yes", `${g.top} / ${g.bottom}`, String(g.operations), String(g.sites)];
}

/** The detail pane of the selected group: Mark, Shape (a spin of 0 to 63), Letter, Description. */
export interface MarkDetail {
  mode: MarkMode;
  shape: number;
  letter: string;
  description: string;
}

export function markDetail(g: DrillGroupRow): MarkDetail {
  return { mode: g.symbol.mode, shape: g.symbol.shape_index ?? 0, letter: g.symbol.letter ?? "", description: g.symbol.description ?? "" };
}

/**
 * `commitDetail`: the detail pane as the group's mark (`SetAssignment`), one `set_drill_profile`; null when it is what the profile already says (`same`: nothing to commit).
 * `assigned` is the profile's own assignment of the group, if it has one.
 */
export function markEditCmd(key: string, d: MarkDetail, assigned: MarkDetail | null): Cmd | null {
  const mark = { mode: d.mode, shape_index: Math.min(63, Math.max(0, Math.trunc(d.shape))), letter: d.letter, description: d.description };
  if (assigned && assigned.mode === mark.mode && assigned.shape === mark.shape_index && assigned.letter === mark.letter && assigned.description === mark.description) return null;
  return { op: "pcb_item", verb: "set_drill_profile", profile: { assignment: { key, mark } } };
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- the canvas

/** `PCB_DRILL_CHART::IsDataRow`: a row that reports on the board (it has a key); a chart written before the keys were recorded: every row between the heading and the totals. */
export function isDataRow(chart: BoardDrillChart, table: BoardTable, row: number): boolean {
  const keys = chart.row_keys ?? {};
  if (Object.keys(keys).length > 0) return keys[String(row)] != null;
  const last = table.rows - 1 - ((chart.show_totals ?? true) ? 1 : 0);
  return row >= 1 && row <= last;
}

/**
 * `PCB_SELECTION_TOOL::selectChartRow`: a double click on a data row of the one selected chart selects that row's cells ("The whole row goes in, because formatting one cell of
 * a generated row and not its neighbours is never what was meant"); null when the click is not on one (the heading or the totals fall through to the chart's dialog).
 */
export function chartRowCells(chart: BoardDrillChart, table: BoardTable, x: number, y: number): string[] | null {
  const clicked = table.cells.find((c) => c.col_span > 0 && c.row_span > 0 && cellHitDistance(c, x, y) <= 0);
  if (!clicked || !isDataRow(chart, table, clicked.row)) return null;
  return table.cells.filter((c) => c.row === clicked.row).map((c) => c.id);
}
