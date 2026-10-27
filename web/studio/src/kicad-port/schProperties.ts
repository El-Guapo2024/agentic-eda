// Properties (`E`, a double-click) for every kind of schematic item: which dialog a selection opens, and the command each dialog's OK sends.
//
// Ported from `SCH_EDIT_TOOL::Properties` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad): the first selected item decides, a selection of lines,
// bus entries and junctions opens the Wire/Bus (or Line, or Junction) dialog only when it holds nothing else, and any other item opens its own dialog
// only when it is the only one selected. The commands are the backend's `edit_label`, `edit_text`, `edit_sheet`, `set_stroke` and `edit_graphic`
// (crates/ops/src/sch_props.rs, sch_edit.rs). A dialog sends only what its user changed -- a verb that would change nothing is refused, and the undo
// stack never records a step that did nothing.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, LabelShape, Schematic, SchematicLabel, SchematicText, Sheet } from "../api/types";
import type { LabelFieldEdit, SchColor, SchGraphic, SchHAlign, SchLineStyle, SchVAlign } from "../api/schEditTypes";
import { textLie } from "./schTextItem";

export type LabelSpinName = "right" | "up" | "left" | "bottom";

/** What `E` opens for a selection. */
export type PropertiesTarget =
  | { kind: "symbol" | "power" | "label" | "text" | "sheet" | "sheet_pin" | "graphic"; id: string }
  /** Wire/Bus Properties, Line Properties or Junction Properties: one dialog for the lines, bus entries and junctions named. */
  | { kind: "stroke"; ids: string[] };

type Kind = "symbol" | "power" | "wire" | "bus" | "bus_entry" | "junction" | "line" | "label" | "text" | "no_connect" | "sheet" | "sheet_pin" | "graphic";

function kinds(sch: Schematic, ids: readonly string[]): Array<{ id: string; kind: Kind }> {
  const wire = new Map(sch.wires.map((w) => [w.id, w]));
  const label = new Set(sch.labels.map((l) => l.id));
  const text = new Set(sch.texts.map((t) => t.id));
  const power = new Set(sch.power_symbols.map((p) => p.id));
  const noConnect = new Set(sch.no_connects.map((n) => n.id));
  const busEntry = new Set(sch.bus_entries.map((b) => b.id));
  const junction = new Set((sch.junctions ?? []).map((j) => j.id));
  const line = new Set((sch.lines ?? []).map((l) => l.id));
  const graphic = new Set((sch.graphics ?? []).map((g) => g.id));
  const sheet = new Set(sch.sheets.map((s) => s.id));
  const sheetPin = new Set(sch.sheets.flatMap((s) => (s.pins ?? []).map((p) => p.id)));
  const symbol = new Set(sch.symbols.map((s) => s.id));
  const out: Array<{ id: string; kind: Kind }> = [];
  for (const id of ids) {
    const w = wire.get(id);
    const kind: Kind | null = symbol.has(id)
      ? "symbol"
      : power.has(id)
        ? "power"
        : w
          ? w.bus
            ? "bus"
            : "wire"
          : label.has(id)
            ? "label"
            : text.has(id)
              ? "text"
              : noConnect.has(id)
                ? "no_connect"
                : busEntry.has(id)
                  ? "bus_entry"
                  : junction.has(id)
                    ? "junction"
                    : line.has(id)
                      ? "line"
                      : sheet.has(id)
                        ? "sheet"
                        : sheetPin.has(id)
                          ? "sheet_pin"
                          : graphic.has(id)
                            ? "graphic"
                            : null;
    if (kind) out.push({ id, kind });
  }
  return out;
}

/** The dialog `SCH_EDIT_TOOL::Properties` opens for the selection `ids` (in selection order), or null when it opens none. */
export function propertiesTarget(sch: Schematic, ids: readonly string[]): PropertiesTarget | null {
  const items = kinds(sch, ids);
  const front = items[0];
  if (!front) return null;
  if (front.kind === "wire" || front.kind === "bus" || front.kind === "bus_entry" || front.kind === "junction" || front.kind === "line") {
    const all = (...of: Kind[]) => items.every((i) => of.includes(i.kind));
    // `OnlyTypes( { graphic line } )` -> DIALOG_LINE_PROPERTIES, `OnlyTypes( { junction } )` -> DIALOG_JUNCTION_PROPS, `OnlyTypes( { wire, bus, bus entry, junction } )`
    // -> DIALOG_WIRE_BUS_PROPERTIES; any other mix opens nothing.
    if (all("line") || all("junction") || all("wire", "bus", "bus_entry", "junction")) return { kind: "stroke", ids: items.map((i) => i.id) };
    return null;
  }
  // `default: if( selection.Size() > 1 ) return 0;`
  if (items.length > 1) return null;
  switch (front.kind) {
    // a power symbol is a symbol in KiCad: `SCH_SYMBOL_T` opens `DIALOG_SYMBOL_PROPERTIES` (with the Spice button hidden for a power one)
    case "symbol":
    case "power":
    case "label":
    case "text":
    case "sheet":
    case "sheet_pin":
    case "graphic":
      return { kind: front.kind, id: front.id };
    default:
      // A no-connect has no properties (`case SCH_NO_CONNECT_T: break;`).
      return null;
  }
}

// ---------------------------------------------------------------------------------------------------------------------------------- units and colours

/** Millimetres typed in a dialog to micrometres; null when it is not a number. */
export function mmToNm(text: string): number | null {
  const v = Number(text.trim().replace(",", "."));
  return text.trim() !== "" && Number.isFinite(v) ? Math.round(v * 1e6) : null;
}

/** Micrometres as the millimetres a dialog shows. */
export function nmToMm(um: number): string {
  return String(Math.round(um) / 1e6);
}

/** `COLOR4D::UNSPECIFIED`: the colour of the item's own layer. */
export const UNSPECIFIED_COLOR: SchColor = { r: 0, g: 0, b: 0, a: 0 };

export function isUnspecified(c: SchColor | null | undefined): boolean {
  return !c || (c.r === 0 && c.g === 0 && c.b === 0 && c.a === 0);
}

/** `#rrggbb` for a colour input. */
export function colorToHex(c: SchColor): string {
  const h = (v: number) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

/** The opaque colour a colour input's `#rrggbb` stands for; null when it is not one. */
export function hexToColor(hex: string): SchColor | null {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
  return m ? { r: parseInt(m[1]!, 16), g: parseInt(m[2]!, 16), b: parseInt(m[3]!, 16), a: 255 } : null;
}

export function sameColor(a: SchColor | null | undefined, b: SchColor | null | undefined): boolean {
  if (isUnspecified(a) && isUnspecified(b)) return true;
  return !!a && !!b && a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
}

// ---------------------------------------------------------------------------------------------------------------------------------- label, text, sheet

/** The label's own fields as Label Properties' grid starts with them: every field but a global label's Sheet References (`SCH_LABEL_BASE::m_fields` past the mandatory one). */
export function ownLabelFields(label: Partial<Pick<SchematicLabel, "fields">>): LabelFieldEdit[] {
  return (label.fields ?? []).filter((f) => !f.intersheet_refs).map((f) => ({ name: f.name, text: f.text, visible: f.visible, italic: f.italic }));
}

/** How a label's field is named in the grid (`SCH_LABEL_BASE::GetDefaultFieldName`): "Net Class" for `Netclass`, "Sheet References" for `Intersheetrefs`. */
export function labelFieldDisplayName(name: string): string {
  return name === "Netclass" ? "Net Class" : name === "Intersheetrefs" ? "Sheet References" : name;
}

/**
 * `DIALOG_LABEL_PROPERTIES::OnAddField`: the new row is a net class unless the label has a field (other than the Sheet References) that is not one, then "Field"; it takes
 * the look of the last row (`SetAttributes`, `SetVisible`), else shown and italic. `isrVisible` is a global label's Sheet References row (the last row when there is no other).
 */
export function newLabelField(fields: readonly LabelFieldEdit[], isrVisible: boolean | null): LabelFieldEdit {
  const name = fields.every((f) => f.name === "Netclass") ? "Netclass" : "Field";
  const last = fields[fields.length - 1];
  if (last) return { name, text: "", visible: last.visible, italic: last.italic };
  if (isrVisible !== null) return { name, text: "", visible: isrVisible, italic: false };
  return { name, text: "", visible: true, italic: true };
}

function sameFields(a: readonly LabelFieldEdit[], b: readonly LabelFieldEdit[]): boolean {
  return a.length === b.length && a.every((f, i) => f.name === b[i]!.name && f.text === b[i]!.text && f.visible === b[i]!.visible && f.italic === b[i]!.italic);
}

/** Label Properties: only the fields that differ from the label as it is (`spin` is the label's current spin, stored or read off its wire). */
export function labelEditCmd(
  label: Pick<SchematicLabel, "id" | "net" | "scope" | "shape"> & Partial<Pick<SchematicLabel, "size_nm" | "bold" | "italic" | "auto_rotate" | "fields">>,
  spin: LabelSpinName,
  next: { text: string; shape: LabelShape; spin: LabelSpinName; sizeNm?: number | null; bold?: boolean; italic?: boolean; autoRotate?: boolean; fields?: LabelFieldEdit[] }
): Cmd | null {
  const edit: { text?: string; shape?: LabelShape; spin?: LabelSpinName; size_nm?: number; bold?: boolean; italic?: boolean; auto_rotate?: boolean; fields?: LabelFieldEdit[] } = {};
  // the grid of the label's own fields, when it changed (the backend drops empty rows and names unnamed ones as the dialog's OK does)
  if (next.fields !== undefined) {
    const canonical = next.fields.map((f) => ({ ...f, name: f.name.trim() === "Net Class" ? "Netclass" : f.name }));
    if (!sameFields(canonical, ownLabelFields(label))) edit.fields = canonical;
  }
  const text = next.text.trim();
  if (text !== label.net) edit.text = text;
  if (label.scope !== "local" && next.shape !== label.shape) edit.shape = next.shape;
  // the "Auto" check (`SetAutoRotateOnPlacement`) is for global and hierarchical labels; while it is on the spin is the connection's, not the radio button's
  const auto = label.scope !== "local" && (next.autoRotate ?? !!label.auto_rotate);
  if (label.scope !== "local" && next.autoRotate !== undefined && next.autoRotate !== !!label.auto_rotate) edit.auto_rotate = next.autoRotate;
  if (!auto && next.spin !== spin) edit.spin = next.spin;
  // the size, bold and italic of the text (`SetTextSize`, `SetBold`, `SetItalic`); a label that was never edited has the default size, regular
  if (next.sizeNm !== undefined && next.sizeNm !== null && next.sizeNm !== (label.size_nm && label.size_nm > 0 ? label.size_nm : 1_270_000)) edit.size_nm = next.sizeNm;
  if (next.bold !== undefined && next.bold !== !!label.bold) edit.bold = next.bold;
  if (next.italic !== undefined && next.italic !== !!label.italic) edit.italic = next.italic;
  return Object.keys(edit).length > 0 ? { op: "sch_edit", verb: "edit_label", id: label.id, ...edit } : null;
}

/**
 * Symbol Properties of a power symbol (`DIALOG_SYMBOL_PROPERTIES::TransferDataFromWindow` for a symbol whose Value is the net it asserts): the new Value (an empty one is not
 * sent: the backend refuses it) and whether the Reference and the Value are shown. One `edit_field` per field that changed, in one command when there are two or more, so the
 * dialog's OK is one undo step; null when nothing changed.
 */
export function powerEditCmd(
  power: { id: string; net: string; fields?: ReadonlyArray<{ name: string; visible: boolean }> },
  next: { net: string; showReference: boolean; showValue: boolean }
): Cmd | null {
  const cmds: Cmd[] = [];
  const field = (name: string) => `fld:${power.id}:${name}`;
  const visible = (name: string) => power.fields?.find((f) => f.name === name)?.visible;
  const edit = (name: string, change: { text?: string; visible?: boolean }): Cmd => ({ op: "sch_edit", verb: "edit_field", id: field(name), ...change });
  const net = next.net.trim();
  const valueChange: { text?: string; visible?: boolean } = {};
  if (net !== power.net) valueChange.text = net;
  if (visible("Value") !== undefined && visible("Value") !== next.showValue) valueChange.visible = next.showValue;
  if (Object.keys(valueChange).length > 0) cmds.push(edit("Value", valueChange));
  if (visible("Reference") !== undefined && visible("Reference") !== next.showReference) cmds.push(edit("Reference", { visible: next.showReference }));
  if (cmds.length === 0) return null;
  return cmds.length === 1 ? cmds[0]! : { op: "batch", cmds };
}

/** Text Properties of a free text: only what differs. `sizeNm` null (not a number) is left out; the backend refuses a size outside 0.01 to 1000 mm. */
export function textEditCmd(
  text: Pick<SchematicText, "id" | "content" | "size_nm" | "angle"> & Partial<Pick<SchematicText, "h" | "v" | "vertical">>,
  next: { content: string; sizeNm: number | null; vertical: boolean; h?: SchHAlign; v?: SchVAlign }
): Cmd | null {
  const edit: { text?: string; size_nm?: number; angle?: number; h?: SchHAlign; v?: SchVAlign } = {};
  if (next.content !== text.content) edit.text = next.content;
  if (next.sizeNm !== null && next.sizeNm !== text.size_nm) edit.size_nm = next.sizeNm;
  // KiCad offers horizontal and vertical (`m_vertical`); a text is only ever one of the two (`KeepUpright`), so a legacy angle reads as the nearer.
  const lie = textLie(text);
  if (next.vertical !== lie.vertical) edit.angle = next.vertical ? 90_000 : 0;
  // how it is justified about its anchor, in its own axes (`m_hAlign*`, `m_vAlign*`)
  if (next.h !== undefined && next.h !== lie.h) edit.h = next.h;
  if (next.v !== undefined && next.v !== lie.v) edit.v = next.v;
  return Object.keys(edit).length > 0 ? { op: "sch_edit", verb: "edit_text", id: text.id, ...edit } : null;
}

/**
 * Sheet Properties: the name and file the sheet has, and the page number, as one command (one undo step). `path` is the sheet's own sheet path
 * (the sheet in view's, then its id): the page is set for that path (`SCH_SHEET_PATH::SetPageNumber` on the instance the dialog edits).
 */
export function sheetEditCmd(sheet: Pick<Sheet, "id" | "name" | "file" | "page">, next: { name: string; file: string; page: string }, path?: string[]): Cmd | null {
  const cmds: Cmd[] = [];
  const name = next.name.trim();
  const file = next.file.trim();
  const edit: { name?: string; file?: string } = {};
  if (name !== sheet.name) edit.name = name;
  if (file !== "" && file !== sheet.file && file !== sheet.file.replace(/\.kicad_sch$/, "")) edit.file = file;
  if (Object.keys(edit).length > 0) cmds.push({ op: "sch_edit", verb: "edit_sheet", id: sheet.id, ...edit });
  const page = next.page.trim();
  if (page !== (sheet.page ?? "")) cmds.push(path ? { op: "set_sheet_page", sheet: sheet.id, page, path } : { op: "set_sheet_page", sheet: sheet.id, page });
  if (cmds.length === 0) return null;
  return cmds.length === 1 ? cmds[0]! : { op: "batch", cmds };
}

// ---------------------------------------------------------------------------------------------------------------------------------- strokes

/** One value across a selection: the value they all have, or "mixed" (`INDETERMINATE_ACTION`). */
export type Shared<T> = { mixed: false; value: T } | { mixed: true };

function shared<T>(values: T[], same: (a: T, b: T) => boolean): Shared<T> {
  const first = values[0];
  if (first === undefined) return { mixed: true };
  return values.every((v) => same(v, first)) ? { mixed: false, value: first } : { mixed: true };
}

/** What a Wire/Bus/Line/Junction dialog shows for the items it edits. */
export interface StrokeView {
  /** The lines, bus entries and graphic lines among the items (they have a width, a style and a colour). */
  hasStroke: boolean;
  /** The junctions among the items (they have a diameter and a colour). */
  hasJunction: boolean;
  widthNm: Shared<number>;
  style: Shared<SchLineStyle>;
  color: Shared<SchColor>;
  diameterNm: Shared<number>;
}

export function strokeView(sch: Schematic, ids: readonly string[]): StrokeView {
  const wire = new Map(sch.wires.map((w) => [w.id, w]));
  const entry = new Map(sch.bus_entries.map((b) => [b.id, b]));
  const line = new Map((sch.lines ?? []).map((l) => [l.id, l]));
  const junction = new Map((sch.junctions ?? []).map((j) => [j.id, j]));
  const widths: number[] = [];
  const styles: SchLineStyle[] = [];
  const colors: SchColor[] = [];
  const diameters: number[] = [];
  const junctionColors: SchColor[] = [];
  for (const id of ids) {
    const w = wire.get(id);
    const b = entry.get(id);
    const l = line.get(id);
    const j = junction.get(id);
    if (w || b || l) {
      const st = (w ?? b ?? l)!.stroke;
      widths.push(l ? l.width_nm : (st?.width_nm ?? 0));
      styles.push(st?.style ?? "default");
      colors.push(st?.color ?? UNSPECIFIED_COLOR);
    } else if (j) {
      diameters.push(j.look?.diameter_nm ?? 0);
      junctionColors.push(j.look?.color ?? UNSPECIFIED_COLOR);
    }
  }
  return {
    hasStroke: widths.length > 0,
    hasJunction: diameters.length > 0,
    widthNm: shared(widths, (a, b) => a === b),
    style: shared(styles, (a, b) => a === b),
    color: shared([...colors, ...junctionColors], sameColor),
    diameterNm: shared(diameters, (a, b) => a === b),
  };
}

/** What the user changed in the stroke dialog (a field left `undefined` is untouched). */
export interface StrokeChange {
  widthNm?: number;
  style?: SchLineStyle;
  color?: SchColor;
  diameterNm?: number;
}

export function strokeEditCmd(ids: readonly string[], change: StrokeChange): Cmd | null {
  const edit: { width_nm?: number; style?: SchLineStyle; color?: SchColor; diameter_nm?: number } = {};
  if (change.widthNm !== undefined) edit.width_nm = Math.max(0, change.widthNm);
  if (change.style !== undefined) edit.style = change.style;
  if (change.color !== undefined) edit.color = isUnspecified(change.color) ? UNSPECIFIED_COLOR : change.color;
  if (change.diameterNm !== undefined) edit.diameter_nm = Math.max(0, change.diameterNm);
  return Object.keys(edit).length > 0 ? { op: "sch_edit", verb: "set_stroke", ids: [...ids], ...edit } : null;
}

// ---------------------------------------------------------------------------------------------------------------------------------- drawn graphics

/** A graphic with every default spelled out, so one that says its defaults and one that leaves them out are the same graphic. */
function normalised(g: SchGraphic): unknown {
  const s = g.shape;
  let shape: unknown = s;
  switch (s.type) {
    case "rectangle":
      shape = { ...s, corner_radius_nm: s.corner_radius_nm ?? 0 };
      break;
    case "text_box":
      shape = { ...s, angle: s.angle ?? 0, bold: !!s.bold, italic: !!s.italic, h_align: s.h_align ?? "left", v_align: s.v_align ?? "top", margin_nm: s.margin_nm ?? 0 };
      break;
    case "rule_area":
      shape = { ...s, exclude_from_sim: !!s.exclude_from_sim, exclude_from_bom: !!s.exclude_from_bom, exclude_from_board: !!s.exclude_from_board, dnp: !!s.dnp };
      break;
    case "directive":
      shape = { ...s, orientation: s.orientation ?? 0, shape: s.shape ?? "round", netclass: s.netclass ?? "", component_class: s.component_class ?? "" };
      break;
    default:
      break;
  }
  const fill = g.fill ?? "none";
  const colour = (c: SchColor | undefined) => (c && !isUnspecified(c) ? c : null);
  return { id: g.id, shape, width_nm: g.width_nm ?? 0, line_style: g.line_style ?? "default", color: colour(g.color), fill, fill_color: fill === "color" ? colour(g.fill_color) : null };
}

/** Is `b` the graphic `a` already is? A dialog sends `edit_graphic` only when it is not (the backend refuses an edit that changes nothing). */
export function graphicsEqual(a: SchGraphic, b: SchGraphic): boolean {
  return stableJson(normalised(a)) === stableJson(normalised(b));
}

/** JSON with the keys of every object in order, so two objects with the same fields compare equal whatever order the fields were added in. */
function stableJson(v: unknown): string {
  return JSON.stringify(v, (_k, value: unknown) =>
    value && typeof value === "object" && !Array.isArray(value)
      ? Object.fromEntries(Object.entries(value as Record<string, unknown>).sort(([x], [y]) => (x < y ? -1 : x > y ? 1 : 0)))
      : value
  );
}
