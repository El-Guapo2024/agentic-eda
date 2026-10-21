// The Properties panel's rows for a board's table and its cells.
//
//   pcbnew/pcb_table.cpp      PCB_TABLE_DESC       Start X / Y, then "Table Properties": External Border, Header Border, Border Width / Style / Color,
//                                                  Row Separators, Cell Separators, Separators Width / Style / Color
//   pcbnew/pcb_tablecell.cpp  PCB_TABLECELL_DESC   the rows of a text box (`PCB_TEXTBOX_DESC`: Knockout, "Margins") and of the text (`EDA_TEXT_DESC`) that a cell
//                                                  keeps, masks the rest (Position, Layer, Net, the shape's), and adds "Table": Column Width, Row Height
//
// Registered in `pcbProperties.ts` (`build()` calls `registerTableProperties`), so the classes sit in the same `PropertyManager` as the rest; this file holds
// what the two classes need and nothing else of the panel. Every setter answers with the table verbs (`TableCmd`, sent as `{ op: "table", verb: ... }`), so a
// grid edit of several cells is one `batch`, one undo step.
//
// A cell does not inherit `EDA_TEXT` here: the generic text rows of the panel read a free text's or a field's record, and a cell's text lives in the table, so the
// cell registers the same rows under its own class (same names, same groups, same order as KiCad shows them).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, BoardTable, BoardTableCell, Cmd, Um } from "../api/types";
import type { TableCellEdit, TableLineStyleName, TablePatch } from "../api/tableTypes";
import { colorToHex, hexToColor, isUnspecified, UNSPECIFIED_COLOR } from "./schProperties";
import type { PropertyManager, PropertyDef, PropValue, Choice } from "./propertyManager";
import { cellById, tableById } from "./tables";

/** What the registration needs of the panel's own module (which imports this one: nothing here is imported back at load time). */
export interface TablePropertyHelpers<I extends { readonly type: string }, C> {
  /** `PropertyManager.addProperty` with the panel's registry: the group is the second argument. */
  add: (def: PropertyDef<I, C, Cmd>, group?: string) => void;
}

/** The two classes' items: a table, and a cell with the table it is in. */
export interface TableItemParts {
  readonly table?: BoardTable;
  readonly cell?: BoardTableCell;
}

type Item = { readonly type: string; readonly id: string } & TableItemParts;

/** The lookups of the panel's `pcbItemOf`: the item a selection id names, if it is a table or one of its cells. */
export function tableItemOf(board: BoardState, id: string): ({ type: "PCB_TABLE" | "PCB_TABLECELL"; id: string } & TableItemParts) | null {
  const table = tableById(board, id);
  if (table) return { type: "PCB_TABLE", id, table };
  const hit = cellById(board, id);
  if (hit) return { type: "PCB_TABLECELL", id, table: hit.table, cell: hit.cell };
  return null;
}

/** Every table and cell of a board, as items (for a selection too big to be searched one id at a time). */
export function tableItemsOf(board: BoardState): ({ type: "PCB_TABLE" | "PCB_TABLECELL"; id: string } & TableItemParts)[] {
  const out: ({ type: "PCB_TABLE" | "PCB_TABLECELL"; id: string } & TableItemParts)[] = [];
  for (const table of board.drawings?.tables ?? []) {
    out.push({ type: "PCB_TABLE", id: table.id, table });
    for (const cell of table.cells) out.push({ type: "PCB_TABLECELL", id: cell.id, table, cell });
  }
  return out;
}

const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const isTable = (i: Item): boolean => i.type === "PCB_TABLE";
const isCell = (i: Item): boolean => i.type === "PCB_TABLECELL";

/** The five styles `ENUM_MAP<LINE_STYLE>` has in the table classes (a stroke of the default style is shown as the first). */
const LINE_STYLES: readonly Choice[] = [
  { label: "Solid", value: "solid" },
  { label: "Dashed", value: "dash" },
  { label: "Dotted", value: "dot" },
  { label: "Dash-Dot", value: "dash_dot" },
  { label: "Dash-Dot-Dot", value: "dash_dot_dot" },
];

const styleShown = (s: TableLineStyleName): string => (s === "default" ? "solid" : s);

/** The colour of a stroke as the colour row holds it (`#rrggbb`, empty for `COLOR4D::UNSPECIFIED`). */
const colorShown = (c: readonly [number, number, number, number] | null): string => (!c || isUnspecified({ r: c[0], g: c[1], b: c[2], a: c[3] }) ? "" : colorToHex({ r: c[0], g: c[1], b: c[2], a: c[3] }));
const colorSent = (text: string): { r: number; g: number; b: number; a: number } => (text === "" ? UNSPECIFIED_COLOR : (hexToColor(text) ?? UNSPECIFIED_COLOR));

/** The command that sets `patch` on a table. */
const setTable = (id: string, patch: TablePatch): Cmd => ({ op: "table", verb: "set", id, patch });
/** The command that edits one cell. */
const editCell = (i: Item, edit: TableCellEdit): Cmd => ({ op: "table", verb: "edit_cells", id: i.table!.id, cells: [[i.cell!.row, i.cell!.col]], edit });

/** `PCB_TABLE_DESC` and `PCB_TABLECELL_DESC`. `Item` is the panel's item type, which carries the table and cell fields of `TableItemParts`. */
export function registerTableProperties<I extends Item, C>(pm: PropertyManager<I, C, Cmd>, h: TablePropertyHelpers<I, C>): void {
  const add = (def: PropertyDef<I, C, Cmd>, group = ""): void => h.add(def, group);

  // ------------------------------------------------------------------------------------------------------------------------- PCB_TABLE
  pm.registerType("PCB_TABLE");
  pm.inheritsAfter("PCB_TABLE", "BOARD_ITEM");
  // A table is on one layer, any layer of the board: `BOARD_ITEM`'s Layer row (pcbProperties.ts) reads and sets it with the rest of the free items'.

  add({
    owner: "PCB_TABLE",
    name: "Start X",
    kind: "int",
    display: "coord",
    get: (i) => i.table!.x,
    available: isTable,
    set: (i, v) => (i.table!.x === Math.round(num(v)) ? [] : [setTable(i.table!.id, { at: { x: Math.round(num(v)), y: i.table!.y } })]),
  });
  add({
    owner: "PCB_TABLE",
    name: "Start Y",
    kind: "int",
    display: "coord",
    get: (i) => i.table!.y,
    available: isTable,
    set: (i, v) => (i.table!.y === Math.round(num(v)) ? [] : [setTable(i.table!.id, { at: { x: i.table!.x, y: Math.round(num(v)) } })]),
  });

  const TABLE = "Table Properties";
  const flag = (name: string, key: "external" | "header" | "rows" | "cols", read: (t: BoardTable) => boolean): void => {
    add(
      {
        owner: "PCB_TABLE",
        name,
        kind: "bool",
        get: (i) => read(i.table!),
        available: isTable,
        set: (i, v) => (read(i.table!) === Boolean(v) ? [] : [setTable(i.table!.id, { props: { [key]: Boolean(v) } })]),
      },
      TABLE
    );
  };
  const stroke = (t: BoardTable, which: "border" | "separators") => (which === "border" ? t.border.stroke : t.separators.stroke);
  const strokeRows = (which: "border" | "separators", prefix: string, widthKey: "border_width_um" | "separators_width_um", styleKey: "border_style" | "separators_style", colorKey: "border_color" | "separators_color"): void => {
    add(
      {
        owner: "PCB_TABLE",
        name: `${prefix} Width`,
        kind: "int",
        display: "size",
        get: (i) => stroke(i.table!, which).width,
        available: isTable,
        set: (i, v) => (stroke(i.table!, which).width === Math.round(num(v)) ? [] : [setTable(i.table!.id, { props: { [widthKey]: Math.round(num(v)) } })]),
      },
      TABLE
    );
    add(
      {
        owner: "PCB_TABLE",
        name: `${prefix} Style`,
        kind: "enum",
        choices: () => LINE_STYLES,
        get: (i) => styleShown(stroke(i.table!, which).style),
        available: isTable,
        set: (i, v) => (styleShown(stroke(i.table!, which).style) === String(v) ? [] : [setTable(i.table!.id, { props: { [styleKey]: String(v) as TableLineStyleName } })]),
      },
      TABLE
    );
    add(
      {
        owner: "PCB_TABLE",
        name: `${prefix} Color`,
        kind: "color",
        get: (i) => colorShown(stroke(i.table!, which).color),
        available: isTable,
        set: (i, v) => (colorShown(stroke(i.table!, which).color) === String(v) ? [] : [setTable(i.table!.id, { props: { [colorKey]: colorSent(String(v)) } })]),
      },
      TABLE
    );
  };
  flag("External Border", "external", (t) => t.border.external);
  flag("Header Border", "header", (t) => t.border.header);
  strokeRows("border", "Border", "border_width_um", "border_style", "border_color");
  flag("Row Separators", "rows", (t) => t.separators.rows);
  flag("Cell Separators", "cols", (t) => t.separators.cols);
  strokeRows("separators", "Separators", "separators_width_um", "separators_style", "separators_color");

  // --------------------------------------------------------------------------------------------------------------------- PCB_TABLECELL
  pm.registerType("PCB_TABLECELL");
  pm.inheritsAfter("PCB_TABLECELL", "BOARD_ITEM");
  // `PCB_TABLECELL_DESC`'s masks: a cell has no position, layer or lock of its own (the table's are), no net, and none of the shape's rows. (KiCad leaves "Locked" on
  // the panel; a cell's lock is its table's, and no verb locks a cell, so the row is not offered.)
  for (const name of ["Position X", "Position Y", "Layer", "Locked"]) pm.mask("PCB_TABLECELL", "BOARD_ITEM", name);

  const TEXT = "Text Properties";
  add(
    {
      owner: "PCB_TABLECELL",
      name: "Text",
      kind: "string",
      get: (i) => i.cell!.text,
      available: isCell,
      set: (i, v) => (i.cell!.text === String(v) ? [] : [editCell(i, { text: String(v) })]),
    },
    TEXT
  );
  // `EDA_TEXT::GetFontProp`: "KiCad Font" is the stroke font, the one a cell has unless the file names another.
  const KICAD_FONT = "KiCad Font";
  add(
    {
      owner: "PCB_TABLECELL",
      name: "Font",
      kind: "string",
      get: (i) => i.cell!.face ?? KICAD_FONT,
      available: isCell,
      set: (i, v) => {
        const face = String(v) === KICAD_FONT ? "" : String(v);
        return (i.cell!.face ?? "") === face ? [] : [editCell(i, { face })];
      },
    },
    TEXT
  );
  add(
    {
      owner: "PCB_TABLECELL",
      name: "Auto Thickness",
      kind: "bool",
      get: (i) => i.cell!.thickness === 0,
      available: isCell,
      // `EDA_TEXT::SetAutoThickness`: on is thickness 0; off keeps the pen the text is drawn with.
      set: (i, v) => ((i.cell!.thickness === 0) === Boolean(v) ? [] : [editCell(i, { thickness_um: Boolean(v) ? 0 : i.cell!.pen })]),
    },
    TEXT
  );
  add(
    {
      owner: "PCB_TABLECELL",
      name: "Thickness",
      kind: "int",
      display: "size",
      // `GetTextThicknessProperty`: the pen the text is drawn with when the thickness is automatic.
      get: (i) => (i.cell!.thickness === 0 ? i.cell!.pen : i.cell!.thickness),
      available: isCell,
      validate: (v) => (num(v) < 0 ? "The thickness cannot be negative." : null),
      set: (i, v) => {
        const w = Math.round(num(v));
        return (i.cell!.thickness === 0 ? i.cell!.pen : i.cell!.thickness) === w ? [] : [editCell(i, { thickness_um: w })];
      },
    },
    TEXT
  );
  const textFlag = (name: "Italic" | "Bold", key: "italic" | "bold"): void => {
    add(
      {
        owner: "PCB_TABLECELL",
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
  // A cell's text is mirrored when its table is on a back layer; no verb sets it on its own, so the row only shows it.
  add({ owner: "PCB_TABLECELL", name: "Mirrored", kind: "bool", get: (i) => i.cell!.mirror, available: isCell }, TEXT);
  const textSize = (name: "Width" | "Height", key: "size_w" | "size_h", edit: "width_um" | "height_um"): void => {
    add(
      {
        owner: "PCB_TABLECELL",
        name,
        kind: "int",
        display: "size",
        get: (i) => i.cell![key],
        available: isCell,
        validate: (v) => (num(v) <= 0 ? "The text size must be positive." : null),
        set: (i, v) => (i.cell![key] === Math.round(num(v)) ? [] : [editCell(i, { [edit]: Math.round(num(v)) })]),
      },
      TEXT
    );
  };
  textSize("Width", "size_w", "width_um");
  textSize("Height", "size_h", "height_um");
  add(
    {
      owner: "PCB_TABLECELL",
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
      owner: "PCB_TABLECELL",
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

  // `PCB_TEXTBOX_DESC`'s "Margins": the four inner margins of the cell.
  const MARGINS = "Margins";
  (["Left", "Top", "Right", "Bottom"] as const).forEach((side, k) => {
    add(
      {
        owner: "PCB_TABLECELL",
        name: `Margin ${side}`,
        kind: "int",
        display: "size",
        get: (i) => i.cell!.margins[k]!,
        available: isCell,
        set: (i, v) => {
          const m = Math.round(num(v));
          if (i.cell!.margins[k] === m) return [];
          const margins: [Um | null, Um | null, Um | null, Um | null] = [null, null, null, null];
          margins[k] = m;
          return [editCell(i, { margins })];
        },
      },
      MARGINS
    );
  });

  // `PCB_TABLECELL_DESC`'s "Table": the width of the cell's column and the height of its row.
  const TABLE_GROUP = "Table";
  add(
    {
      owner: "PCB_TABLECELL",
      name: "Column Width",
      kind: "int",
      display: "size",
      get: (i) => i.table!.col_widths[i.cell!.col]!,
      available: isCell,
      validate: (v) => (num(v) <= 0 ? "The column width must be positive." : null),
      set: (i, v) => ((i.table!.col_widths[i.cell!.col] ?? 0) === Math.round(num(v)) ? [] : [editCell(i, { col_width_um: Math.round(num(v)) })]),
    },
    TABLE_GROUP
  );
  add(
    {
      owner: "PCB_TABLECELL",
      name: "Row Height",
      kind: "int",
      display: "size",
      get: (i) => i.table!.row_heights[i.cell!.row]!,
      available: isCell,
      validate: (v) => (num(v) <= 0 ? "The row height must be positive." : null),
      set: (i, v) => ((i.table!.row_heights[i.cell!.row] ?? 0) === Math.round(num(v)) ? [] : [editCell(i, { row_height_um: Math.round(num(v)) })]),
    },
    TABLE_GROUP
  );
}
