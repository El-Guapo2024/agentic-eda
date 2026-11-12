// The Pin Table's Import (KiCad 25ec2335 `eeschema/dialogs/dialog_lib_edit_pin_table.cpp::OnImportButtonClick`, `common/table_io.cpp::ReadTableFromFileOrClipboard`,
// `common/io/csv.cpp::AutoDecodeCSV`, `common/column_formatter.cpp`): a CSV or TSV table -- a file or the clipboard's text -- whose first row names the columns
// (`GetColTypeForString`: the Pin Table's column labels, any case) and whose other rows are one new pin each (`new SCH_PIN( &aSym )`, then `UpdatePin` of each
// named cell, without units and with True / False). Columns it does not know are ignored and reported ("Unknown columns in data: %s. These columns will be
// ignored."), as is a Visible cell that is no boolean. The dialog then asks (OK / Cancel, "Errors") when there is something to report, and replaces the
// table's pins or appends to them.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibrarySymbolPin } from "../api/types";
import { addedPin } from "./pinTable";
import { COL, COL_COUNT, COLUMN_LABELS, updatePin, type GridContext } from "./pinTableGrid";

/**
 * `AutoDecodeCSV`: a tab anywhere makes it tab-separated, else comma-separated (`rapidcsv` with cells trimmed, `#` comment lines and empty lines skipped, quoted
 * cells with `""` for a quote and line breaks kept); every row as wide as the first. `null` when the first row is empty (nothing read).
 */
export function autoDecodeCsv(input: string): string[][] | null {
  const delimiter = input.includes("\t") ? "\t" : ",";
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  let cellWasQuoted = false;
  let lineStart = true;
  let skipLine = false;
  const trim = (s: string) => s.replace(/^[ \t]+|[ \t]+$/g, "");
  const endCell = () => {
    row.push(cellWasQuoted ? cell : trim(cell));
    cell = "";
    cellWasQuoted = false;
  };
  const endRow = () => {
    const blank = row.length === 0 && !cellWasQuoted && trim(cell) === "";
    endCell();
    // `skipEmptyLines`
    if (!blank) rows.push(row);
    row = [];
    lineStart = true;
  };
  for (let i = 0; i < input.length; i++) {
    const c = input[i]!;
    if (lineStart) {
      lineStart = false;
      skipLine = c === "#";
    }
    if (skipLine) {
      if (c === "\n") {
        skipLine = false;
        lineStart = true;
        row = [];
        cell = "";
      }
      continue;
    }
    if (quoted) {
      if (c === '"') {
        if (input[i + 1] === '"') {
          cell += '"';
          i++;
        } else quoted = false;
      } else cell += c;
      continue;
    }
    if (c === '"' && trim(cell) === "") {
      quoted = true;
      cellWasQuoted = true;
      cell = "";
    } else if (c === delimiter) endCell();
    else if (c === "\r") continue;
    else if (c === "\n") endRow();
    else if (!cellWasQuoted) cell += c;
  }
  if (!lineStart || cell !== "" || row.length > 0) endRow();
  const width = rows[0]?.length ?? 0;
  const out = rows.map((r) => Array.from({ length: width }, (_, j) => r[j] ?? ""));
  return out.length > 0 && width > 0 ? out : null;
}

/** `GetColTypeForString`: the column a header names (`MatchTranslationOrNative`, not case-sensitive), `COL_COUNT` for none. */
export function colTypeForString(label: string): number {
  const at = COLUMN_LABELS.findIndex((l) => l.toLowerCase() === label.toLowerCase());
  return at < 0 ? COL_COUNT : at;
}

/**
 * `OnImportButtonClick` from the table read: with at least a header and one row, a new pin per row -- `new SCH_PIN( &aSym )` (no number or name, unspecified,
 * a line, pointing right, at the origin, 100 mil long, 50 mil texts, unit and body style 0) with each cell of a known column put in by `UpdatePin` -- and the
 * messages to show ("Unknown columns ..." first, then a Visible cell's). `ctx.parseLength` reads lengths without units in the frame's units.
 */
export function importPins(table: readonly (readonly string[])[] | null, ctx: GridContext): { pins: LibrarySymbolPin[]; messages: string[] } {
  const messages: string[] = [];
  const pins: LibrarySymbolPin[] = [];
  if (!table || table.length < 2) return { pins, messages };
  const headerCols = table[0]!.map(colTypeForString);
  const unknown = table[0]!.filter((_, j) => headerCols[j]! >= COL_COUNT);
  // `AccumulateDescriptions`: joined by ", "
  if (unknown.length > 0) messages.push(`Unknown columns in data: ${unknown.join(", ")}. These columns will be ignored.`);
  for (let i = 1; i < table.length; i++) {
    const cols = table[i]!;
    let pin = addedPin([]);
    const maxCol = Math.min(headerCols.length, cols.length);
    for (let j = 0; j < maxCol; j++) {
      const col = headerCols[j]!;
      // `if( headerCols[j] == COL_COUNT ) continue;` -- and Count is no column a pin has (`UpdatePin`'s default case)
      if (col >= COL_COUNT || col === COL.PIN_COUNT) continue;
      pin = updatePin(pin, cols[j]!, col, ctx, (m) => messages.push(m));
    }
    pins.push(pin);
  }
  return { pins, messages };
}

/** The pins after an import: the new ones in place of the table's (`m_rbReplaceAll`) or after them. */
export function importedPinList<T>(current: readonly T[], added: readonly T[], replaceAll: boolean): T[] {
  return replaceAll ? [...added] : [...current, ...added];
}
