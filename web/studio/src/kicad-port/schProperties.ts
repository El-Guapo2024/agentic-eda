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
import type { SchColor, SchGraphic, SchLineStyle } from "../api/schEditTypes";

export type LabelSpinName = "right" | "up" | "left" | "bottom";

/** What `E` opens for a selection. */
export type PropertiesTarget =
  | { kind: "symbol" | "label" | "text" | "sheet" | "graphic"; id: string }
  /** Wire/Bus Properties, Line Properties or Junction Properties: one dialog for the lines, bus entries and junctions named. */
  | { kind: "stroke"; ids: string[] };

type Kind = "symbol" | "power" | "wire" | "bus" | "bus_entry" | "junction" | "line" | "label" | "text" | "no_connect" | "sheet" | "graphic";

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
    case "symbol":
    case "label":
    case "text":
    case "sheet":
    case "graphic":
      return { kind: front.kind, id: front.id };
    default:
      // A power symbol is a symbol in KiCad (its Value is the rail); this studio's symbol dialog has no power symbol yet. A no-connect has no properties (`case SCH_NO_CONNECT_T: break;`).
      return null;
  }
}

// ---------------------------------------------------------------------------------------------------------------------------------- units and colours

/** Millimetres typed in a dialog to micrometres; null when it is not a number. */
export function mmToUm(text: string): number | null {
  const v = Number(text.trim().replace(",", "."));
  return text.trim() !== "" && Number.isFinite(v) ? Math.round(v * 1000) : null;
}

/** Micrometres as the millimetres a dialog shows. */
export function umToMm(um: number): string {
  return String(Math.round(um) / 1000);
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

/** Label Properties: only the fields that differ from the label as it is (`spin` is the label's current spin, stored or read off its wire). */
export function labelEditCmd(
  label: Pick<SchematicLabel, "id" | "net" | "scope" | "shape"> & Partial<Pick<SchematicLabel, "size_um" | "bold" | "italic">>,
  spin: LabelSpinName,
  next: { text: string; shape: LabelShape; spin: LabelSpinName; sizeUm?: number | null; bold?: boolean; italic?: boolean }
): Cmd | null {
  const edit: { text?: string; shape?: LabelShape; spin?: LabelSpinName; size_um?: number; bold?: boolean; italic?: boolean } = {};
  const text = next.text.trim();
  if (text !== label.net) edit.text = text;
  if (label.scope !== "local" && next.shape !== label.shape) edit.shape = next.shape;
  if (next.spin !== spin) edit.spin = next.spin;
  // the size, bold and italic of the text (`SetTextSize`, `SetBold`, `SetItalic`); a label that was never edited has the default size, regular
  if (next.sizeUm !== undefined && next.sizeUm !== null && next.sizeUm !== (label.size_um && label.size_um > 0 ? label.size_um : 1270)) edit.size_um = next.sizeUm;
  if (next.bold !== undefined && next.bold !== !!label.bold) edit.bold = next.bold;
  if (next.italic !== undefined && next.italic !== !!label.italic) edit.italic = next.italic;
  return Object.keys(edit).length > 0 ? { op: "sch_edit", verb: "edit_label", id: label.id, ...edit } : null;
}

/** Text Properties of a free text: only what differs. `sizeUm` null (not a number) is left out; the backend refuses a size outside 0.01 to 1000 mm. */
export function textEditCmd(text: Pick<SchematicText, "id" | "content" | "size_um" | "angle">, next: { content: string; sizeUm: number | null; vertical: boolean }): Cmd | null {
  const edit: { text?: string; size_um?: number; angle?: number } = {};
  if (next.content !== text.content) edit.text = next.content;
  if (next.sizeUm !== null && next.sizeUm !== text.size_um) edit.size_um = next.sizeUm;
  // KiCad offers horizontal and vertical (`m_vertical`); a text at some other angle keeps it until the user picks the other orientation.
  const wasVertical = Math.round(text.angle) === 90;
  if (next.vertical !== wasVertical) edit.angle = next.vertical ? 90_000 : 0;
  return Object.keys(edit).length > 0 ? { op: "sch_edit", verb: "edit_text", id: text.id, ...edit } : null;
}

/** Sheet Properties: the name and file the sheet has, and the page number, as one command (one undo step). */
export function sheetEditCmd(sheet: Pick<Sheet, "id" | "name" | "file" | "page">, next: { name: string; file: string; page: string }): Cmd | null {
  const cmds: Cmd[] = [];
  const name = next.name.trim();
  const file = next.file.trim();
  const edit: { name?: string; file?: string } = {};
  if (name !== sheet.name) edit.name = name;
  if (file !== "" && file !== sheet.file && file !== sheet.file.replace(/\.kicad_sch$/, "")) edit.file = file;
  if (Object.keys(edit).length > 0) cmds.push({ op: "sch_edit", verb: "edit_sheet", id: sheet.id, ...edit });
  const page = next.page.trim();
  if (page !== (sheet.page ?? "")) cmds.push({ op: "set_sheet_page", sheet: sheet.id, page });
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
  widthUm: Shared<number>;
  style: Shared<SchLineStyle>;
  color: Shared<SchColor>;
  diameterUm: Shared<number>;
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
      widths.push(l ? l.width_um : (st?.width_um ?? 0));
      styles.push(st?.style ?? "default");
      colors.push(st?.color ?? UNSPECIFIED_COLOR);
    } else if (j) {
      diameters.push(j.look?.diameter_um ?? 0);
      junctionColors.push(j.look?.color ?? UNSPECIFIED_COLOR);
    }
  }
  return {
    hasStroke: widths.length > 0,
    hasJunction: diameters.length > 0,
    widthUm: shared(widths, (a, b) => a === b),
    style: shared(styles, (a, b) => a === b),
    color: shared([...colors, ...junctionColors], sameColor),
    diameterUm: shared(diameters, (a, b) => a === b),
  };
}

/** What the user changed in the stroke dialog (a field left `undefined` is untouched). */
export interface StrokeChange {
  widthUm?: number;
  style?: SchLineStyle;
  color?: SchColor;
  diameterUm?: number;
}

export function strokeEditCmd(ids: readonly string[], change: StrokeChange): Cmd | null {
  const edit: { width_um?: number; style?: SchLineStyle; color?: SchColor; diameter_um?: number } = {};
  if (change.widthUm !== undefined) edit.width_um = Math.max(0, change.widthUm);
  if (change.style !== undefined) edit.style = change.style;
  if (change.color !== undefined) edit.color = isUnspecified(change.color) ? UNSPECIFIED_COLOR : change.color;
  if (change.diameterUm !== undefined) edit.diameter_um = Math.max(0, change.diameterUm);
  return Object.keys(edit).length > 0 ? { op: "sch_edit", verb: "set_stroke", ids: [...ids], ...edit } : null;
}

// ---------------------------------------------------------------------------------------------------------------------------------- drawn graphics

/** A graphic with every default spelled out, so one that says its defaults and one that leaves them out are the same graphic. */
function normalised(g: SchGraphic): unknown {
  const s = g.shape;
  let shape: unknown = s;
  switch (s.type) {
    case "rectangle":
      shape = { ...s, corner_radius_um: s.corner_radius_um ?? 0 };
      break;
    case "text_box":
      shape = { ...s, angle: s.angle ?? 0, bold: !!s.bold, italic: !!s.italic, h_align: s.h_align ?? "left", v_align: s.v_align ?? "top", margin_um: s.margin_um ?? 0 };
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
  return { id: g.id, shape, width_um: g.width_um ?? 0, line_style: g.line_style ?? "default", color: colour(g.color), fill, fill_color: fill === "color" ? colour(g.fill_color) : null };
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
