// The Properties panel's rows for a sheet's table and its cells.
//
//   eeschema/sch_table.cpp      SCH_TABLE_DESC      Start X / Y, then "Table Properties": External Border, Header Border, Border Width / Style / Color, Row Separators, Cell
//                                                   Separators, Separators Width / Style / Color (the same rows as the board's table: `registerTableClass`)
//   eeschema/sch_tablecell.cpp  SCH_TABLECELL_DESC  the rows of a text box (`SCH_TEXTBOX_DESC`: the four margins, "Text Size") and of the text (`EDA_TEXT_DESC`) that a cell keeps --
//                                                   its masks are the shape's rows and the text's Width, Height, Thickness, Orientation, Mirrored, Visible and Hyperlink -- and adds
//                                                   "Table": Column Width, Row Height and "Cell Properties": Background Fill, Background Fill Color
//
// Registered in `schItemProperties.ts` (`build()` calls `registerSchTableProperties`). Every setter answers with the table verbs (`TableCmd`, sent as `{ op: "sch_table", verb:
// ... }`), so a grid edit of several cells is one `batch`, one undo step.
//
// A cell does not inherit `EDA_TEXT` here: the generic text rows of the panel read a text's or a text box's record, and a cell's text lives in the table, so the cell registers the
// same rows under its own class (same names, same groups, same order as KiCad shows them).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardTableCell, Cmd, Schematic, Nm } from "../api/types";
import type { TableCellEdit } from "../api/tableTypes";
import { colorToHex, hexToColor, isUnspecified } from "./schProperties";
import type { PropertyManager, PropertyDef, PropValue } from "./propertyManager";
import { registerTableClass, type TableItemParts, type TablePropertyHelpers } from "./pcbTableProperties";
import { cellById, tableById } from "./tables";

type Item = { readonly type: string; readonly id: string } & TableItemParts;

/** The lookups of the panel's `schItemOf`: the item a selection id names, if it is a table or one of its cells. */
export function schTableItemOf(sch: Pick<Schematic, "tables">, id: string): ({ type: "SCH_TABLE" | "SCH_TABLECELL"; id: string } & TableItemParts) | null {
  const holder = { drawings: { tables: sch.tables ?? [] } };
  const table = tableById(holder, id);
  if (table) return { type: "SCH_TABLE", id, table };
  const hit = cellById(holder, id);
  if (hit) return { type: "SCH_TABLECELL", id, table: hit.table, cell: hit.cell };
  return null;
}

const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const isCell = (i: Item): boolean => i.type === "SCH_TABLECELL";

const FILL_NONE = "none";

/** The colour of a fill as the colour row holds it (`#rrggbb`, empty when the cell has none). */
const fillColorShown = (c: BoardTableCell): string => {
  const v = c.fill_color;
  return !v || isUnspecified({ r: v[0], g: v[1], b: v[2], a: v[3] }) ? "" : colorToHex({ r: v[0], g: v[1], b: v[2], a: v[3] });
};

/** The command that edits one cell. */
const editCell = (i: Item, edit: TableCellEdit): Cmd => ({ op: "sch_table", verb: "edit_cells", id: i.table!.id, cells: [[i.cell!.row, i.cell!.col]], edit }) as Cmd;

/** `SCH_TABLE_DESC` and `SCH_TABLECELL_DESC`. `Item` is the panel's item type, which carries the table and cell fields of `TableItemParts`. */
export function registerSchTableProperties<I extends Item, C>(pm: PropertyManager<I, C, Cmd>, h: TablePropertyHelpers<I, C>): void {
  const add = (def: PropertyDef<I, C, Cmd>, group = ""): void => h.add(def, group);
  registerTableClass(pm, h, { table: "SCH_TABLE", base: "SCH_ITEM", op: "sch_table" });

  // --------------------------------------------------------------------------------------------------------------------- SCH_TABLECELL
  pm.registerType("SCH_TABLECELL");
  pm.inheritsAfter("SCH_TABLECELL", "SCH_ITEM");
  // A cell has no lock of its own (a lock is the table's: the file knows only `(locked yes)` of the `(table`).
  pm.mask("SCH_TABLECELL", "SCH_ITEM", "Locked");

  const TEXT = "Text Properties";
  add(
    {
      owner: "SCH_TABLECELL",
      name: "Text",
      kind: "string",
      get: (i) => i.cell!.text,
      available: isCell,
      set: (i, v) => (i.cell!.text === String(v) ? [] : [editCell(i, { text: String(v) })]),
    },
    TEXT
  );
  const textFlag = (name: "Italic" | "Bold", key: "italic" | "bold"): void => {
    add(
      {
        owner: "SCH_TABLECELL",
        name,
        kind: "bool",
        get: (i) => i.cell![key],
        available: isCell,
        set: (i, v) => (i.cell![key] === Boolean(v) ? [] : [editCell(i, { [key]: Boolean(v) })]),
      },
      TEXT
    );
  };
  textFlag("Italic", "italic");
  textFlag("Bold", "bold");
  add(
    {
      owner: "SCH_TABLECELL",
      name: "Horizontal Justification",
      kind: "enum",
      choices: () => [
        { label: "Left", value: "left" },
        { label: "Center", value: "center" },
        { label: "Right", value: "right" },
      ],
      get: (i) => i.cell!.h_align,
      available: isCell,
      set: (i, v) => (i.cell!.h_align === v ? [] : [editCell(i, { h_align: String(v) as BoardTableCell["h_align"] })]),
    },
    TEXT
  );
  add(
    {
      owner: "SCH_TABLECELL",
      name: "Vertical Justification",
      kind: "enum",
      choices: () => [
        { label: "Top", value: "top" },
        { label: "Center", value: "center" },
        { label: "Bottom", value: "bottom" },
      ],
      get: (i) => i.cell!.v_align,
      available: isCell,
      set: (i, v) => (i.cell!.v_align === v ? [] : [editCell(i, { v_align: String(v) as BoardTableCell["v_align"] })]),
    },
    TEXT
  );
  // `SCH_TEXTBOX::SetSchTextSize`: both dimensions of the glyph, one number.
  add(
    {
      owner: "SCH_TABLECELL",
      name: "Text Size",
      kind: "int",
      display: "size",
      get: (i) => i.cell!.size_h,
      available: isCell,
      // `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM`: "Don't allow text to disappear" (0.01 to 1000 mm)
      validate: (v) => (num(v) < 10_000 ? "The text size must be at least 0.01 mm." : num(v) > 1_000_000_000 ? "Value must be less than or equal to 1000 mm" : null),
      set: (i, v) => {
        const size = Math.round(num(v));
        return i.cell!.size_h === size && i.cell!.size_w === size ? [] : [editCell(i, { width_nm: size, height_nm: size })];
      },
    },
    TEXT
  );

  // `SCH_TEXTBOX_DESC`'s "Margins": the four inner margins of the cell.
  const MARGINS = "Margins";
  (["Left", "Top", "Right", "Bottom"] as const).forEach((side, k) => {
    add(
      {
        owner: "SCH_TABLECELL",
        name: `Margin ${side}`,
        kind: "int",
        display: "size",
        get: (i) => i.cell!.margins[k]!,
        available: isCell,
        set: (i, v) => {
          const m = Math.round(num(v));
          if (i.cell!.margins[k] === m) return [];
          const margins: [Nm | null, Nm | null, Nm | null, Nm | null] = [null, null, null, null];
          margins[k] = m;
          return [editCell(i, { margins })];
        },
      },
      MARGINS
    );
  });

  // `SCH_TABLECELL_DESC`'s "Table": the width of the cell's column and the height of its row.
  const TABLE_GROUP = "Table";
  add(
    {
      owner: "SCH_TABLECELL",
      name: "Column Width",
      kind: "int",
      display: "size",
      get: (i) => i.table!.col_widths[i.cell!.col]!,
      available: isCell,
      validate: (v) => (num(v) <= 0 ? "The column width must be positive." : null),
      set: (i, v) => ((i.table!.col_widths[i.cell!.col] ?? 0) === Math.round(num(v)) ? [] : [editCell(i, { col_width_nm: Math.round(num(v)) })]),
    },
    TABLE_GROUP
  );
  add(
    {
      owner: "SCH_TABLECELL",
      name: "Row Height",
      kind: "int",
      display: "size",
      get: (i) => i.table!.row_heights[i.cell!.row]!,
      available: isCell,
      validate: (v) => (num(v) <= 0 ? "The row height must be positive." : null),
      set: (i, v) => ((i.table!.row_heights[i.cell!.row] ?? 0) === Math.round(num(v)) ? [] : [editCell(i, { row_height_nm: Math.round(num(v)) })]),
    },
    TABLE_GROUP
  );

  // "Cell Properties": the background (`EDA_SHAPE::SetFilled`, `SetFillColor`). Filling is `FILLED_SHAPE`, the outline's colour, unless the cell has a colour of its own; the
  // colour makes the fill a fill of that colour, and an empty one takes the fill away.
  const CELL_PROPS = "Cell Properties";
  add(
    {
      owner: "SCH_TABLECELL",
      name: "Background Fill",
      kind: "bool",
      get: (i) => (i.cell!.fill ?? FILL_NONE) !== FILL_NONE,
      available: isCell,
      set: (i, v) => {
        const on = Boolean(v);
        if (((i.cell!.fill ?? FILL_NONE) !== FILL_NONE) === on) return [];
        return [editCell(i, { fill: on ? (i.cell!.fill_color ? "color" : "outline") : "none" })];
      },
    },
    CELL_PROPS
  );
  add(
    {
      owner: "SCH_TABLECELL",
      name: "Background Fill Color",
      kind: "color",
      get: (i) => fillColorShown(i.cell!),
      available: isCell,
      set: (i, v) => {
        const text = String(v);
        if (fillColorShown(i.cell!) === text) return [];
        if (text === "") return [editCell(i, { fill: "none" })];
        const c = hexToColor(text);
        return c ? [editCell(i, { fill: "color", fill_color: { r: c.r, g: c.g, b: c.b, a: c.a } })] : [];
      },
    },
    CELL_PROPS
  );
}
