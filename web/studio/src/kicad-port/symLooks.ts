// What a library symbol's graphic or field has beyond its kind's members (UI-LEFT-2 YY2): a shape's Private flag, border and fill colours, hatched fill and a rectangle's
// corner radius, a text's or text box's hyperlink, a line style and a text's pen; a field's face and colour. The server keeps them beside the item in the symbol's
// `kicad` overlay (`crates/model/src/lib_keep.rs` `LibrarySymbolKeep::graphic_looks`, `stroke_types`, `text_styles`, `field_looks`), reads and writes them where KiCad's
// `SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol` has them (`formatRect` .. `formatPoly`, `formatFill`, `STROKE_PARAMS::Format`, `EDA_TEXT::Format`), and sets them with
// `set_symbol_graphic_looks` / `set_symbol_field_looks`, which take the whole state of the item (so a caller sends what it read with its change in).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, LibrarySymbol, LibrarySymbolGraphic } from "../api/types";
import type { SchColor } from "../api/schEditTypes";

/** `eda_model::lib_keep::KeptGraphicLooks`. */
export interface GraphicLooks {
  private?: boolean;
  stroke_color?: SchColor | null;
  fill_color?: SchColor | null;
  /** `hatch`, `reverse_hatch` or `cross_hatch`: the shape's `fill` is then `color`. */
  hatch?: string | null;
  radius_mm?: number;
  hyperlink?: string;
}

/** `eda_model::lib_keep::KeptFieldLooks`. */
export interface FieldLooks {
  font?: string | null;
  color?: SchColor | null;
  /** The pen, mm; null is Auto Thickness. */
  thickness_mm?: number | null;
}

/** The whole state `set_symbol_graphic_looks` takes. `lineStyle` null is `default` (drawn and shown as Solid); `thicknessMm` null is Auto Thickness. */
export interface GraphicState {
  looks: GraphicLooks;
  lineStyle: string | null;
  thicknessMm: number | null;
}

interface Keep {
  graphic_looks?: Record<string, GraphicLooks>;
  stroke_types?: Record<string, string>;
  text_styles?: Record<string, { thickness_mm?: number | null }>;
  field_looks?: Record<string, FieldLooks>;
}

const keepOf = (sym: LibrarySymbol): Keep => (sym.kicad ?? {}) as Keep;

/** `COLOR4D::UNSPECIFIED`, which a file writes `0 0 0 0`. */
export const isUnspecified = (c: SchColor | null | undefined): boolean => !c || (c.r === 0 && c.g === 0 && c.b === 0 && c.a === 0);

/** The looks, the line style and the pen of the graphic `id` as the symbol keeps them. */
export function graphicStateOf(sym: LibrarySymbol, id: string): GraphicState {
  const k = keepOf(sym);
  return { looks: { ...(k.graphic_looks?.[id] ?? {}) }, lineStyle: k.stroke_types?.[id] ?? null, thicknessMm: k.text_styles?.[id]?.thickness_mm ?? null };
}

export function fieldLooksOf(sym: LibrarySymbol, name: string): FieldLooks {
  return { ...(keepOf(sym).field_looks?.[name] ?? {}) };
}

/** The looks with the defaults left out, as the server compares them (`KeptGraphicLooks::is_default`). */
function trimmed(l: GraphicLooks): GraphicLooks {
  const out: GraphicLooks = {};
  if (l.private) out.private = true;
  if (!isUnspecified(l.stroke_color)) out.stroke_color = l.stroke_color;
  if (!isUnspecified(l.fill_color)) out.fill_color = l.fill_color;
  if (l.hatch) out.hatch = l.hatch;
  if (l.radius_mm && l.radius_mm > 0) out.radius_mm = l.radius_mm;
  if (l.hyperlink) out.hyperlink = l.hyperlink;
  return out;
}

const same = (a: GraphicState, b: GraphicState) => JSON.stringify(trimmed(a.looks)) === JSON.stringify(trimmed(b.looks)) && (a.lineStyle ?? null) === (b.lineStyle ?? null) && (a.thicknessMm ?? null) === (b.thicknessMm ?? null);

/** The verb that gives the graphic `id` the state `next` (`id` "" is the graphic an `add_symbol_graphic` before it added), or nothing when it already has it. */
export function graphicLooksCmd(libId: string, id: string, next: GraphicState, before?: GraphicState): Cmd[] {
  if (before && same(before, next)) return [];
  return [{ op: "set_symbol_graphic_looks", lib_id: libId, id, looks: trimmed(next.looks), line_style: next.lineStyle, thickness_mm: next.thicknessMm }];
}

/** The state with `patch` over it, as a command, or nothing when it changes nothing. */
export function graphicLooksWith(libId: string, sym: LibrarySymbol, id: string, patch: Partial<GraphicState> & { looks?: Partial<GraphicLooks> }): Cmd[] {
  const before = graphicStateOf(sym, id);
  const next: GraphicState = { looks: { ...before.looks, ...(patch.looks ?? {}) }, lineStyle: patch.lineStyle !== undefined ? patch.lineStyle : before.lineStyle, thicknessMm: patch.thicknessMm !== undefined ? patch.thicknessMm : before.thicknessMm };
  return graphicLooksCmd(libId, id, next, before);
}

export function fieldLooksWith(libId: string, sym: LibrarySymbol, name: string, patch: Partial<FieldLooks>): Cmd[] {
  const before = fieldLooksOf(sym, name);
  const next: FieldLooks = { ...before, ...patch };
  const clean = (l: FieldLooks): FieldLooks => ({ ...(l.font ? { font: l.font } : {}), ...(!isUnspecified(l.color) ? { color: l.color } : {}), ...(l.thickness_mm ? { thickness_mm: l.thickness_mm } : {}) });
  if (JSON.stringify(clean(before)) === JSON.stringify(clean(next))) return [];
  return [{ op: "set_symbol_field_looks", lib_id: libId, name, looks: clean(next) }];
}

/** Whether a graphic is "Private to Symbol Editor": a text's and a text box's own flag, a shape's kept beside it. */
export function isPrivateGraphic(sym: LibrarySymbol, g: LibrarySymbolGraphic): boolean {
  if (g.kind === "text" || g.kind === "text_box") return !!g.private;
  return !!(g.id && keepOf(sym).graphic_looks?.[g.id]?.private);
}

/** A copy's state, taken with it by Duplicate and Paste: the original's, or none. */
export function carriedStateOf(sym: LibrarySymbol | null | undefined, id: string | undefined): GraphicState | null {
  if (!sym || !id) return null;
  const s = graphicStateOf(sym, id);
  return Object.keys(trimmed(s.looks)).length === 0 && s.lineStyle === null && s.thicknessMm === null ? null : s;
}

/** `lineTypeNames` (common/stroke_params.cpp): the border Style choices of Shape Properties and Text Box Properties, in their order. The default style shows as the
 *  first, Solid (`DEFAULT_LINE_STYLE_LABEL`), and OK writes the one chosen. */
export const LINE_STYLE_CHOICES: readonly { value: string; label: string }[] = [
  { value: "solid", label: "Solid" },
  { value: "dash", label: "Dashed" },
  { value: "dot", label: "Dotted" },
  { value: "dash_dot", label: "Dash-Dot" },
  { value: "dash_dot_dot", label: "Dash-Dot-Dot" },
];
