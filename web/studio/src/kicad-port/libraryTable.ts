// The rules behind Manage Symbol Libraries, Manage Footprint Libraries, Manage Design Block Libraries and Configure Paths, as pure functions (the dialogs are components/libraryTables/*).
//
// Ported from KiCad (commit 8303b2ad): the grid and what OK does with it -- `common/libraries/lib_table_grid_data_model.cpp` (the columns, `SetValue` giving a row
// the format its path says), `common/lib_table_grid_tricks.cpp` (`VerifyTable`, Delete, Move Up / Down, Activate and Show of the selected rows),
// `eeschema/dialogs/panel_sym_lib_table.cpp`, `pcbnew/dialogs/panel_fp_lib_table.cpp` and `common/dialogs/panel_design_block_lib_table.cpp` (the nickname a file makes, the duplicate warning,
// `populateEnvironReadOnlyTable`: the path substitutions), `common/lib_id.cpp` (the characters a nickname may not hold) -- and Configure Paths:
// `common/dialogs/dialog_configure_paths.cpp` and `common/validators.cpp` (`ENV_VAR_NAME_VALIDATOR`). The server checks the same things again before it writes
// anything (`crates/kicad/src/library_table.rs`, `crates/cli/src/library_tables.rs`); doing them here too lets the dialog put the cursor in the cell at fault.
import { libraryNicknameIllegalChar } from "./libraryNames";

export type TableKind = "symbol" | "footprint" | "design_block";
export type TableScope = "global" | "project" | "nested";

/** One row of a library table, as the grid has it: `enabled` is the Enable box, `visible` the Show box (symbol libraries only). */
export interface TableRow {
  nickname: string;
  uri: string;
  /** The library format: `KiCad`, `Legacy`, ..., `Table` for a chained table. */
  type: string;
  options: string;
  descr: string;
  enabled: boolean;
  visible: boolean;
  /** What the server found out about it: `ok`, `table` (a chained table, which opens in a tab of its own) or `error`. */
  status?: "ok" | "error" | "table";
  error?: string;
  /** The row's path with its variables replaced. */
  path?: string;
  /** The rows of the table a `Table` row chains, and whether its file can be written (its tab is editable then). */
  table?: { path: string; rows: TableRow[]; writable?: boolean };
}

/** The editable columns of the grid, in the order of `COL_ORDER` (the status column and the two check boxes come first). */
export type Column = "nickname" | "uri" | "type" | "options" | "descr";

export const TABLE_TYPE = "Table";
export const KICAD_TYPE = "KiCad";

export function libraryExtension(kind: TableKind): string {
  return kind === "symbol" ? ".kicad_sym" : kind === "footprint" ? ".pretty" : ".kicad_blocks";
}

export function tableFileName(kind: TableKind): string {
  return kind === "symbol" ? "sym-lib-table" : kind === "footprint" ? "fp-lib-table" : "design-block-lib-table";
}

/** The dialog's title: `DIALOG_EDIT_LIBRARY_TABLES dlg( aParent, _( "Symbol Libraries" ) )` (panel_sym_lib_table.cpp::InvokeSchEditSymbolLibTable), "Footprint Libraries"
 *  (panel_fp_lib_table.cpp::InvokePcbLibTableEditor), "Design Block Libraries" (panel_design_block_lib_table.cpp::InvokeEditDesignBlockLibTable); the actions that open them are
 *  the "Manage ... Libraries..." ones. */
export function tableTitle(kind: TableKind): string {
  return kind === "symbol" ? "Symbol Libraries" : kind === "footprint" ? "Footprint Libraries" : "Design Block Libraries";
}

export function emptyRow(): TableRow {
  return { nickname: "", uri: "", type: "", options: "", descr: "", enabled: true, visible: true };
}

/** `LIB_ID::FixIllegalChars( name, true )`: each character a nickname may not hold becomes `_`. */
export function fixNickname(name: string): string {
  let out = "";
  for (const c of name) out += c.charCodeAt(0) < 0x20 || c === "\\" || c === ":" ? "_" : c;
  return out;
}

/**
 * The nickname a library file or folder makes (`browseLibrariesHandler`, `PCB_BASE_EDIT_FRAME::AddLibrary`, `DESIGN_BLOCK_PANE::AddDesignBlockLibrary`): the file's name
 * without its extension; for a footprint library that is not a `.pretty` folder, or a design block library that is not a `.kicad_blocks` folder, the whole name.
 */
export function nicknameForFile(kind: TableKind, fileName: string): string {
  const dot = fileName.lastIndexOf(".");
  const stem = dot > 0 ? fileName.slice(0, dot) : fileName;
  const ext = dot > 0 ? fileName.slice(dot) : "";
  if (kind !== "symbol" && ext !== libraryExtension(kind)) return fixNickname(fileName);
  return fixNickname(stem);
}

/** The last component of a path (either separator). */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const i = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return i < 0 ? trimmed : trimmed.slice(i + 1);
}

/** `SYMBOL_LIBRARY_MANAGER::GetUniqueLibraryName`: `New_Library`, else `New_Library_0`, `New_Library_1`, ... */
export function uniqueLibraryName(taken: readonly string[]): string {
  const base = "New_Library";
  if (!taken.includes(base)) return base;
  for (let i = 0; ; i++) if (!taken.includes(`${base}_${i}`)) return `${base}_${i}`;
}

/** `GuessPluginTypeFromLibPath` as far as the name goes: a library table file is a chained table, KiCad's own formats are told by their extension -- for symbols the
 *  `.kicad_symdir` folder of KiCad 10 too (`SCH_IO_KICAD_SEXPR::CanReadLibrary`: a folder of `.kicad_sym` files). */
export function guessType(kind: TableKind, path: string): string | null {
  const name = baseName(path);
  if (name === "sym-lib-table" || name === "fp-lib-table" || name === "design-block-lib-table") return TABLE_TYPE;
  if (name.endsWith(libraryExtension(kind))) return KICAD_TYPE;
  if (kind === "symbol" && name.endsWith(".kicad_symdir")) return KICAD_TYPE;
  return null;
}

/** `LIB_TABLE_GRID_DATA_MODEL::SetValue`: the cell takes the text; a new path also gives the row the format it says. */
export function setCell(row: TableRow, column: Column, value: string, kind: TableKind): TableRow {
  const next: TableRow = { ...row, [column]: value };
  if (column === "uri") {
    const guess = guessType(kind, value);
    if (guess) next.type = guess;
  }
  // what the server found out belongs to the old text
  if (column === "uri" || column === "type" || column === "options") {
    delete next.status;
    delete next.error;
    delete next.path;
  }
  return next;
}

/** What a row says is wrong with it, for the status column's tooltip. */
export function statusTitle(row: TableRow): string {
  if (row.status === "error") return row.error ?? "";
  if (row.status === "table" || row.type === TABLE_TYPE) return "Open library table";
  return "";
}

/** `LIB_TABLE_GRID_TRICKS::AppendRowHandler`: an empty row at the end. */
export function appendRow(rows: readonly TableRow[]): TableRow[] {
  return [...rows, emptyRow()];
}

/** `DeleteRowHandler`: the selected rows go (the one with the cursor when none is selected). */
export function deleteRows(rows: readonly TableRow[], selected: readonly number[]): TableRow[] {
  const gone = new Set(selected);
  return rows.filter((_, i) => !gone.has(i));
}

/** `MoveUpHandler` / `MoveDownHandler`: the row with the cursor swaps places with its neighbour; at an end nothing moves. Returns the rows and where the row is now. */
export function moveRow(rows: readonly TableRow[], at: number, delta: -1 | 1): { rows: TableRow[]; at: number } {
  const to = at + delta;
  if (at < 0 || at >= rows.length || to < 0 || to >= rows.length) return { rows: [...rows], at };
  const next = [...rows];
  const moved = next[at] as TableRow;
  next[at] = next[to] as TableRow;
  next[to] = moved;
  return { rows: next, at: to };
}

/** The popup menu's Activate / Deactivate Selected and Set / Unset Visible Flag. */
export function setFlag(rows: readonly TableRow[], selected: readonly number[], flag: "enabled" | "visible", value: boolean): TableRow[] {
  const chosen = new Set(selected);
  return rows.map((r, i) => (chosen.has(i) ? { ...r, [flag]: value } : r));
}

/** What the popup menu offers for the selected rows: each flag can be turned the way some row of the selection does not have it. */
export function flagOffers(rows: readonly TableRow[], selected: readonly number[]): { activate: boolean; deactivate: boolean; show: boolean; hide: boolean } {
  const picked = selected.map((i) => rows[i]).filter((r): r is TableRow => !!r);
  return { activate: picked.some((r) => !r.enabled), deactivate: picked.some((r) => r.enabled), show: picked.some((r) => !r.visible), hide: picked.some((r) => r.visible) };
}

/** One thing OK refused: the row, the column to put the cursor in and the message. */
export interface TableProblem {
  row: number;
  column: Column;
  message: string;
}

/**
 * `LIB_TABLE_GRID_TRICKS::VerifyTable`: a row with no path is dropped without a word; one with no nickname, or with a character a nickname may not hold, stops the
 * dialog; so do two rows with one nickname (the later is the one reported, counted among the rows as shown). The trimmed values are what is kept, and a table with
 * no Show column keeps no hidden flag. Returns the cleaned rows or the first problem.
 */
export function verifyRows(rows: readonly TableRow[], supportsVisibility: boolean): { rows: TableRow[] } | { problem: TableProblem } {
  const kept: { row: TableRow; at: number }[] = [];
  for (let at = 0; at < rows.length; at++) {
    const source = rows[at] as TableRow;
    const nickname = source.nickname.trim();
    const uri = source.uri.trim();
    if (uri === "") continue;
    if (nickname === "") return { problem: { row: at, column: "nickname", message: "Library must have a nickname." } };
    const illegal = libraryNicknameIllegalChar(nickname);
    if (illegal) return { problem: { row: at, column: "nickname", message: `Illegal character '${illegal}' in nickname '${nickname}'.` } };
    const reserved = reservedNicknameMessage(nickname);
    if (reserved) return { problem: { row: at, column: "nickname", message: reserved } };
    kept.push({ row: { ...source, nickname, uri, visible: supportsVisibility ? source.visible : true }, at });
  }
  for (let i = 0; i < kept.length; i++) {
    const later = kept.slice(i + 1).find((k) => k.row.nickname === (kept[i] as { row: TableRow }).row.nickname);
    if (later) return { problem: { row: later.at, column: "nickname", message: `Multiple libraries cannot share the same nickname ('${(kept[i] as { row: TableRow }).row.nickname}').` } };
  }
  return { rows: kept.map((k) => k.row) };
}

/** The nickname of the studio's own project library (`eda:Name`): a table library of that name would be shadowed by it. */
export const RESERVED_NICKNAME = "eda";

export function reservedNicknameMessage(nickname: string): string | null {
  return nickname === RESERVED_NICKNAME ? `The nickname '${RESERVED_NICKNAME}' is the studio's own project library (the ids eda:Name): choose another.` : null;
}

/** KiCad's library table has no spelling for a double quote or a line break in a value; the server refuses them too. */
export function unwritable(rows: readonly TableRow[]): TableProblem | null {
  const fields: [Column, string][] = [
    ["nickname", "A nickname"],
    ["uri", "A library path"],
    ["type", "A library format"],
    ["options", "Options"],
    ["descr", "A description"],
  ];
  for (let at = 0; at < rows.length; at++) {
    for (const [column, label] of fields) {
      if (/["\n\r]/.test((rows[at] as TableRow)[column])) return { row: at, column, message: `${label} cannot hold a double quote or a line break: KiCad's library table has no way to write one.` };
    }
  }
  return null;
}

/** The fields that make two tables the same table (what the server's `LIBRARY_TABLE::operator==` compares), so OK knows whether anything changed. */
export function sameRows(a: readonly TableRow[], b: readonly TableRow[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((r, i) => {
    const o = b[i] as TableRow;
    return r.nickname === o.nickname && r.uri === o.uri && r.type === o.type && r.options === o.options && r.descr === o.descr && r.enabled === o.enabled && r.visible === o.visible;
  });
}

// ------------------------------------------------------------------------------------------------ path substitutions

/** The variable names a text uses (`${NAME}` and `$(NAME)`), in order of first use. */
export function variablesUsed(text: string): string[] {
  const found: string[] = [];
  const re = /\$\{([^}]+)\}|\$\(([^)]+)\)/g;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    const name = m[1] ?? m[2] ?? "";
    if (name !== "" && !found.includes(name)) found.push(name);
  }
  return found;
}

export interface VariableValue {
  name: string;
  value: string;
}

/**
 * `populateEnvironReadOnlyTable`: the "Available path substitutions" under the grid -- every variable the paths of the tables use, plus `KIPRJMOD` and the folder
 * variable of the kind of library (`KICAD10_SYMBOL_DIR`), each with the value it has, sorted by name. `values` are the variables that have one.
 */
export function pathSubstitutions(uris: readonly string[], kind: TableKind, major: number, values: readonly VariableValue[]): VariableValue[] {
  const folder = kind === "symbol" ? "SYMBOL" : kind === "footprint" ? "FOOTPRINT" : "DESIGN_BLOCK";
  const names = new Set<string>(["KIPRJMOD", `KICAD${major}_${folder}_DIR`]);
  for (const uri of uris) for (const n of variablesUsed(uri)) names.add(n);
  const byName = new Map(values.map((v) => [v.name, v.value]));
  return [...names].sort().map((name) => ({ name, value: byName.get(name) ?? "" }));
}

// ------------------------------------------------------------------------------------------------ Configure Paths

/** `ENV_VAR_NAME_VALIDATOR`: capital letters, digits (never first) and `_`; typing a lower-case letter gives the capital. */
export function fixVariableName(typed: string): string {
  let out = "";
  for (const c of typed.toUpperCase()) if (c === "_" || (c >= "A" && c <= "Z") || (c >= "0" && c <= "9" && out !== "")) out += c;
  return out;
}

export function isVariableName(name: string): boolean {
  return name !== "" && fixVariableName(name) === name;
}

export interface VariableProblem {
  row: number;
  column: "name" | "value";
  message: string;
}

/**
 * `DIALOG_CONFIGURE_PATHS::TransferDataFromWindow` and `OnGridCellChanging`, for the board's own variables: a row with nothing in it is skipped; a name needs
 * a path and a path a name; `KIPRJMOD` is reserved; two rows cannot share a name. Returns the cleaned variables or the first problem.
 */
export function validateVariables(rows: readonly VariableValue[]): { variables: VariableValue[] } | { problem: VariableProblem } {
  const out: VariableValue[] = [];
  for (let row = 0; row < rows.length; row++) {
    const name = (rows[row] as VariableValue).name.trim();
    const value = (rows[row] as VariableValue).value.trim();
    if (name === "" && value === "") continue;
    if (name === "") return { problem: { row, column: "name", message: "Environment variable name cannot be empty." } };
    if (value === "") return { problem: { row, column: "value", message: "Environment variable path cannot be empty." } };
    if (name === "KIPRJMOD") return { problem: { row, column: "name", message: "The name KIPRJMOD is reserved, and cannot be used." } };
    if (!isVariableName(name)) return { problem: { row, column: "name", message: `'${name}' is not a variable name: capital letters, digits (not first) and underscores only.` } };
    if (/["\n\r]/.test(value)) return { problem: { row, column: "value", message: "A path cannot hold a double quote or a line break." } };
    if (out.some((v) => v.name === name)) return { problem: { row, column: "name", message: `Two variables are named ${name}.` } };
    out.push({ name, value });
  }
  return { variables: out };
}

export function sameVariables(a: readonly VariableValue[], b: readonly VariableValue[]): boolean {
  return a.length === b.length && a.every((v, i) => v.name === (b[i] as VariableValue).name && v.value === (b[i] as VariableValue).value);
}

// ------------------------------------------------------------------------------------------------ Edit Options (`DIALOG_PLUGIN_OPTIONS`)

/** `LIBRARY_TABLE`'s `OPT_SEP`: the options of a row are `name=value` pairs joined by `|`, a `|` in a value escaped with `\`. */
export const OPT_SEP = "|";

/**
 * `LIBRARY_TABLE::ParseOptions`: the pairs of a row's options -- white space before a pair skipped, `\|` a `|` of the value, the first `=` between the name and the value,
 * a pair with no `=` an option present with no value. The answer is a `std::map`: sorted by name, a name given twice keeping its later value.
 */
export function parseOptions(list: string): [string, string][] {
  const props = new Map<string, string>();
  const s = [...list];
  let i = 0;
  while (i < s.length) {
    let pair = "";
    while (i < s.length && /\s/.test(s[i] as string)) i++;
    while (i < s.length) {
      if (s[i] === "\\" && i + 1 < s.length && s[i + 1] === OPT_SEP) {
        i++;
        pair += s[i++];
      } else if (s[i] === OPT_SEP) {
        i++;
        break;
      } else {
        pair += s[i++];
      }
    }
    if (pair !== "") {
      const eq = pair.indexOf("=");
      if (eq >= 0) props.set(pair.slice(0, eq), pair.slice(eq + 1));
      else props.set(pair, "");
    }
  }
  return [...props.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
}

/** `LIBRARY_TABLE::FormatOptions`: the pairs, sorted by name, `name=value` (a name alone when the value is empty), a `|` in a value escaped. */
export function formatOptions(pairs: readonly (readonly [string, string])[]): string {
  const props = new Map<string, string>();
  for (const [k, v] of pairs) props.set(k, v);
  return [...props.entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([k, v]) => (v === "" ? k : `${k}=${v.split(OPT_SEP).join(`\\${OPT_SEP}`)}`))
    .join(OPT_SEP);
}

/** `DIALOG_PLUGIN_OPTIONS::TransferDataFromWindow`: every grid row with a name (both cells trimmed) into the map, then `FormatOptions`. */
export function optionsFromGrid(rows: readonly { name: string; value: string }[]): string {
  return formatOptions(rows.map((r) => [r.name.trim(), r.value.trim()] as [string, string]).filter(([n]) => n !== ""));
}

export interface OptionChoice {
  name: string;
  /** The plugin's own help, in its markup (`<b>`). */
  help: string;
}

/** `INITIAL_HELP` of `DIALOG_PLUGIN_OPTIONS`. */
export const OPTIONS_INITIAL_HELP = "Select an <b>Option Choice</b> in the listbox above, and then click the <b>Append Selected Option</b> button.";

/**
 * The options a library format takes (`IO_BASE::GetLibraryOptions` and its overrides): the KiCad and most symbol formats none (`SCH_IO::GetLibraryOptions`), CADSTAR
 * `csa` and `fplib` (`SCH_IO_CADSTAR_ARCHIVE::GetLibraryOptions`), and every footprint format the six of `PCB_IO::GetLibraryOptions`. In the order of the `std::map`
 * KiCad lists them from.
 */
export function libraryOptionChoices(kind: TableKind, type: string): OptionChoice[] {
  if (kind === "symbol") {
    if (type !== "CADSTAR Schematic Archive") return [];
    return [
      {
        name: "csa",
        help: "Path to the CADSTAR schematic archive (*.csa) file related to this CADSTAR parts library. If none specified it is assumed to be 'symbol.csa' in the same folder.",
      },
      {
        name: "fplib",
        help: "Name of the footprint library related to the symbols in this library. You should create a separate entry for the CADSTAR PCB Archive (*.cpa) file in the footprint library tables. If none specified, 'cadstarpcblib' is assumed.",
      },
    ];
  }
  if (kind !== "footprint" || type === TABLE_TYPE) return [];
  return [
    { name: "debug_level", help: "Enable <b>debug</b> logging for Footprint*() functions in this PCB_IO." },
    { name: "enable_transaction_logging", help: "Enable transaction logging. The mere presence of this option turns on the logging, no need to set a Value." },
    { name: "password", help: "Password for <b>login</b> to some special library server." },
    { name: "python_footprint_plugin", help: "Enter the python module which implements the PCB_IO::Footprint*() functions." },
    { name: "read_filter_regex", help: "Regular expression <b>footprint name</b> filter." },
    { name: "username", help: "User name for <b>login</b> to some special library server." },
  ];
}

/** `LIB_TABLE_GRID_TRICKS::showPopupMenu`: "Edit Options..." is offered for one selected row that is not a chained table. */
export function canEditOptions(rows: readonly TableRow[], selected: readonly number[]): boolean {
  return selected.length === 1 && rows[selected[0] as number] !== undefined && (rows[selected[0] as number] as TableRow).type !== TABLE_TYPE;
}

// ------------------------------------------------------------------------------------------------ Reset Libraries, Migrate Libraries

/** `PANEL_SYM_LIB_TABLE::onReset` / `PANEL_FP_LIB_TABLE::onReset`'s question, asked only when the global table has rows. */
export const RESET_QUESTION = "This action will reset your global library table on disk and cannot be undone.";

/** What Migrate Libraries says when none of the rows chosen is of another format. */
export const MIGRATE_NONE = "Select one or more rows containing libraries to save as current KiCad format.";

/**
 * The rows Migrate Libraries works on: the selected ones (the row with the cursor when none is), less those already in KiCad's format -- and, for symbols, the Database ones
 * (`onConvertLegacyLibraries`: `!= databaseType && != kicadType`; the footprint and design block panels skip only `kicadType`).
 */
export function migratableRows(kind: TableKind, rows: readonly TableRow[], selected: readonly number[], cursor: number): number[] {
  const chosen = selected.length > 0 ? selected : cursor >= 0 ? [cursor] : [];
  return chosen.filter((i) => {
    const r = rows[i];
    return r !== undefined && r.type !== KICAD_TYPE && !(kind === "symbol" && r.type === "Database");
  });
}

/** The question before Migrate Libraries goes on (`IsOK`). */
export function migrateQuestion(kind: TableKind, nicknames: readonly string[]): string {
  if (kind === "symbol") {
    return nicknames.length === 1
      ? `Save '${nicknames[0]}' as current KiCad format (*.kicad_sym) and replace legacy entry in table?`
      : `Save ${nicknames.length} libraries as current KiCad format (*.kicad_sym) and replace legacy entries in table?`;
  }
  return nicknames.length === 1 ? `Save '${nicknames[0]}' as current KiCad format and replace entry in table?` : `Save ${nicknames.length} libraries as current KiCad format and replace entries in table?`;
}

/** The question for a destination that is there already (title "Migrate Library", Yes / No / Cancel). */
export function migrateOverwriteQuestion(kind: TableKind, dest: string): string {
  if (kind === "symbol") return `File '${dest}' already exists. Do you want overwrite this file?`;
  return `Folder '${dest}' already exists. Do you want overwrite any existing ${kind === "footprint" ? "footprints" : "design blocks"}?`;
}
