// The item a schematic placing tool carries, drawn in the page: `SCH_DRAWING_TOOLS::TwoClickPlace`'s `updatePreview` and `PlaceSymbol`'s `addSymbol`
// (`m_view->ClearPreview(); m_view->AddToPreview( item, false )` on every motion and every change, eeschema/tools/sch_drawing_tools.cpp at 8303b2ad) keep the
// item in the tool's own view, so it appears, follows the cursor and turns with no round trip. The studio's painters draw a label, a text, a directive label, a
// power symbol and a symbol from what the page holds -- the carried item's properties (kicad-port/schPlaceTool.ts) and the library symbol's drawing (the
// sheet's `lib_symbols`, or the one the chooser loaded) -- as `GET /api/schematic` would list the placed item.
//
// What the page cannot know is asked of the server's drawing of the placing command (`POST /api/sch/place_preview`) and laid over it when it comes back:
//   * a symbol's and a power symbol's fields where `AutoplaceFields( nullptr, AUTOPLACE_AUTO )` puts them (the engine's autoplacer, `eda_engine::fields`), and
//     a power symbol's id (the server numbers it);
//   * a global label's Sheet References (the pages its name is on);
//   * a text's `${..}` variables (`SCH_TEXT::GetShownText`).
// Until it comes back -- and after a turn, until the server's drawing of the turned item does -- the fields of the last drawing ride with the item, turned with
// it about its anchor as `SCH_SYMBOL::Rotate` / `SetOrientation` turn a symbol's fields; a label shows no Sheet References meanwhile, and a text its raw text.
//
// Everything is drawn at `PREVIEW_AT`, where the server draws it too; the canvas shifts the drawing to the cursor (`withPaste`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibFill, LibGraphic, LibPin, LibSymbol, LibSymbols, LibraryFill, LibrarySymbol, LibrarySymbolGraphic, PinKind, PowerSymbol, SchField, SchematicLabel, SchematicPin, SchematicSymbol, SchematicText } from "../api/types";
import type { SchGraphic } from "../api/schEditTypes";
import { bezierPolyline, SYMBOL_BEZIER_MAX_ERROR_NM } from "./bezierPoly";
import type { SchPastePreview } from "./schClipboard";
import { PREVIEW_AT, symbolOrientation, type CarriedItem, type CarriedSymbol } from "./schPlaceTool";

/** The ids the page gives a carried label, text and directive label (the server numbers its own; a carried symbol rides under its reference). */
export const CARRIED_IDS = { label: "carry:label", text: "carry:text", directive: "carry:directive", power: "carry:power" } as const;

/** The server's drawing of a carried item, and the item it drew. */
export interface ServerDrawing {
  item: CarriedItem;
  preview: SchPastePreview;
}

export interface CarriedPreviewContext {
  /** The sheet's library symbols (`lib_symbols`): a symbol the sheet draws is drawn as the sheet draws it. */
  libSymbols: LibSymbols;
  /** The carried symbol's own drawing, when the sheet has none yet (the one the chooser loaded). */
  lib?: LibSymbol | null;
  /** The server's last drawing of the carried item, if any. */
  server?: ServerDrawing | null;
}

const EMPTY: SchPastePreview = { symbols: [], power_symbols: [], wires: [], labels: [], texts: [], sheets: [], no_connects: [], bus_entries: [], junctions: [], lines: [], graphics: [], groups: [], tables: [], lib_symbols: {} };

const AT: [number, number] = [PREVIEW_AT[0], PREVIEW_AT[1]];

// ------------------------------------------------------------------------------------------------------------- library symbols

const pt = (p: { x: number; y: number }): [number, number] => [p.x, p.y];

/** A library fill as the sheet's painter reads it (`LibFill` has every `LibraryFill`). */
const fillOf = (f: LibraryFill): LibFill => f;

/**
 * A library symbol as the Symbol Editor and the choosers load it (`LibrarySymbol`, `GET /api/symbol`) in the shape the sheet's painter draws a placed one by
 * (`lib_symbols`' `LibSymbol`, `crates/cli/src/studio.rs` `lib_symbol_json`), as `LibrarySymbolGraphic::to_engine_graphics` lowers it for a placed symbol: a
 * private text or text box draws nothing there (`SCH_PAINTER::draw( SCH_TEXT )`: `if( aText->IsPrivate() && !m_schSettings.m_IsSymbolEditor ) return`), a
 * bezier is the polyline `BEZIER_POLY::GetPoly` makes of it, a text box is its box (and, where the sheet's painter draws a library text box's text, its text).
 */
export function libSymbolOfLibrary(sym: LibrarySymbol): LibSymbol {
  const graphics: LibGraphic[] = [];
  for (const g of sym.graphics) {
    const unit = g.unit ?? 0;
    const body_style = g.body_style ?? 0;
    switch (g.kind) {
      case "rectangle":
        graphics.push({ kind: "rectangle", start: pt(g.start), end: pt(g.end), stroke_width: g.stroke_mm, fill: fillOf(g.fill), unit, body_style });
        break;
      case "polyline":
        graphics.push({ kind: "polyline", pts: g.pts.map(pt), stroke_width: g.stroke_mm, fill: fillOf(g.fill), unit, body_style });
        break;
      case "circle":
        graphics.push({ kind: "circle", center: pt(g.center), radius: g.radius_mm, stroke_width: g.stroke_mm, fill: fillOf(g.fill), unit, body_style });
        break;
      case "arc":
        graphics.push({ kind: "arc", start: pt(g.start), mid: pt(g.mid), end: pt(g.end), stroke_width: g.stroke_mm, fill: fillOf(g.fill), unit, body_style });
        break;
      case "bezier": {
        // mm, the curve cut at the symbol's error (`SYMBOL_BEZIER_MAX_ERROR_NM`)
        const poly = bezierPolyline(pt(g.start), pt(g.c1), pt(g.c2), pt(g.end), SYMBOL_BEZIER_MAX_ERROR_NM / 1e6);
        graphics.push({ kind: "polyline", pts: poly, stroke_width: g.stroke_mm, fill: fillOf(g.fill), unit, body_style });
        break;
      }
      case "text":
        if (g.private) break;
        graphics.push({ kind: "text", content: g.text, at: pt(g.at), angle_deg: g.angle_deg, size_mm: g.size_mm, unit, body_style });
        break;
      case "text_box":
        if (g.private) break;
        // the border and the fill are the box; no border (a negative width) and no fill draws nothing of it
        if (g.stroke_mm < 0 && g.fill === "none") break;
        graphics.push(textBoxGraphic(g, unit, body_style));
        break;
    }
  }
  const pins: LibPin[] = sym.pins.map((p) => ({
    number: p.number,
    name: p.name === "~" ? "" : p.name,
    electrical_type: p.electrical_type,
    shape: p.shape,
    at: pt(p.at),
    angle_deg: p.angle_deg,
    length_mm: p.length_mm,
    unit: p.unit ?? 0,
    body_style: p.body_style ?? 0,
    hidden: !!p.hidden,
    ...(p.alternates && p.alternates.length > 0 ? { alternates: p.alternates } : {}),
  }));
  return {
    graphics,
    pins,
    pin_names_hidden: sym.pin_names_hidden,
    pin_numbers_hidden: sym.pin_numbers_hidden,
    pin_name_offset: sym.pin_name_offset_mm,
    body_style_count: sym.has_alternate_body_style ? 2 : Math.max(1, sym.body_style_names?.length ?? 1),
    unit_count: sym.unit_count,
    description: sym.description,
    keywords: sym.keywords,
  };
}

/** A library text box as a placed symbol's drawing has it: its box, the text it holds riding along for a painter that draws it. */
function textBoxGraphic(g: Extract<LibrarySymbolGraphic, { kind: "text_box" }>, unit: number, body_style: number): LibGraphic {
  return { kind: "rectangle", start: pt(g.start), end: pt(g.end), stroke_width: g.stroke_mm, fill: g.fill === "none" ? "none" : "background", unit, body_style };
}

// ------------------------------------------------------------------------------------------------------------- the carried items

/** `SCH_PIN`'s electrical type as `GET /api/schematic`'s pin `kind`. */
function pinKind(t: LibPin["electrical_type"]): PinKind {
  return t === "power_in" || t === "power_out" ? "power" : t === "passive" ? "passive" : t === "no_connect" ? "nc" : "signal";
}

/** `0` is shared by every unit and body style. */
const drawnIn = (itemUnit: number, itemStyle: number, unit: number, style: number) => (itemUnit === 0 || itemUnit === unit) && (itemStyle === 0 || itemStyle === style);

/** The orientation matrix of a placed symbol's `(rot, mirror)` (transform.ts's `SetOrientation` matrices), as a function of a vector. */
function orient(rotDeg: number, mirror: "x" | "y" | null): (v: [number, number]) => [number, number] {
  // KiCad's matrices: rotation first, then the mirror (`MIRROR_X` negates y, `MIRROR_Y` negates x)
  const q = ((Math.round(rotDeg / 90) % 4) + 4) % 4;
  return (v) => {
    let [x, y] = v;
    for (let i = 0; i < q; i++) [x, y] = [y, -x];
    if (mirror === "x") y = -y;
    else if (mirror === "y") x = -x;
    return [x, y];
  };
}

/** The inverse of `orient` (each matrix is orthonormal: its inverse is its transpose). */
function unorient(rotDeg: number, mirror: "x" | "y" | null): (v: [number, number]) => [number, number] {
  const f = orient(rotDeg, mirror);
  const ex = f([1, 0]);
  const ey = f([0, 1]);
  return ([x, y]) => [ex[0] * x + ex[1] * y, ey[0] * x + ey[1] * y];
}

/**
 * The fields of the server's drawing of an earlier state of the item, made to ride with the item as it is now: turned about the anchor by the turn between
 * the two orientations (`SCH_SYMBOL::SetOrientation` turns the fields with the symbol; their text stays as it reads), and with the texts of now.
 */
export function carriedFields(fields: readonly SchField[], was: { rotDeg: number; mirror: "x" | "y" | null }, now: { rotDeg: number; mirror: "x" | "y" | null }, texts: Readonly<Record<string, string>>): SchField[] {
  const back = unorient(was.rotDeg, was.mirror);
  const fwd = orient(now.rotDeg, now.mirror);
  return fields.map((f) => {
    const rel: [number, number] = [f.at[0] - AT[0], f.at[1] - AT[1]];
    const [x, y] = fwd(back(rel));
    const text = texts[f.name];
    return { ...f, at: [AT[0] + x, AT[1] + y], ...(text !== undefined && text !== f.text ? { text, shown: undefined } : {}) };
  });
}

/** The library symbol a carried symbol or power symbol is drawn by: the sheet's, else its own, else the server's. */
function libOf(libId: string, ctx: CarriedPreviewContext): LibSymbol | null {
  return ctx.libSymbols[libId] ?? ctx.lib ?? ctx.server?.preview.lib_symbols[libId] ?? null;
}

function symbolPreview(item: CarriedSymbol, ctx: CarriedPreviewContext): SchPastePreview {
  const lib = libOf(item.libId, ctx);
  const o = symbolOrientation(item);
  const rotDeg = o.rotMillideg / 1000;
  const style = item.bodyStyle ?? 1;
  const pins: SchematicPin[] = (lib?.pins ?? []).filter((p) => drawnIn(p.unit, p.body_style, item.unit, style)).map((p) => ({ number: p.number, name: p.name, kind: pinKind(p.electrical_type) }));
  const sym: SchematicSymbol = {
    id: item.ref,
    reference: item.ref,
    lib_id: item.libId,
    at: AT,
    rot: rotDeg,
    mirror: o.mirror,
    unit: item.unit,
    body_style: style,
    value: item.value,
    mpn: null,
    package: null,
    footprint: item.footprint || null,
    datasheet: null,
    pins,
    ...(item.attrs ?? {}),
  };
  // the fields where the server's autoplacer put them: this very state's, else the last state's turned with the item
  const server = ctx.server?.item.kind === "symbol" && ctx.server.item.libId === item.libId ? ctx.server : null;
  const drawn = server?.preview.symbols.find((s) => s.id === (server.item as CarriedSymbol).ref) ?? server?.preview.symbols[0];
  if (server && drawn?.fields) {
    const was = server.item as CarriedSymbol;
    const same = sameItem(was, item);
    const wasO = symbolOrientation(was);
    sym.fields = same ? drawn.fields : carriedFields(drawn.fields, { rotDeg: wasO.rotMillideg / 1000, mirror: wasO.mirror }, { rotDeg, mirror: o.mirror }, { Reference: item.ref, Value: item.value, Footprint: item.footprint });
    if (drawn.fields_autoplaced !== undefined) sym.fields_autoplaced = drawn.fields_autoplaced;
    if (same && drawn.shown) sym.shown = drawn.shown;
  }
  return { ...EMPTY, symbols: [sym], lib_symbols: lib ? { [item.libId]: lib } : {} };
}

function powerPreview(item: Extract<CarriedItem, { kind: "power" }>, ctx: CarriedPreviewContext): SchPastePreview {
  const lib = libOf(item.libId, ctx);
  const libPin = lib?.pins[0];
  const server = ctx.server?.item.kind === "power" && ctx.server.item.libId === item.libId ? ctx.server : null;
  const drawn = server?.preview.power_symbols[0];
  const ps: PowerSymbol = {
    id: drawn?.id ?? CARRIED_IDS.power,
    lib_id: item.libId,
    at: AT,
    rot: item.rotMillideg / 1000,
    net: item.net,
    pin: { number: libPin?.number ?? "1", name: libPin?.name ?? item.net, kind: "power" },
  };
  if (server && drawn) {
    const was = server.item as typeof item;
    ps.reference = drawn.reference;
    if (drawn.props) ps.props = drawn.props;
    if (drawn.fields) ps.fields = sameItem(was, item) ? drawn.fields : carriedFields(drawn.fields, { rotDeg: was.rotMillideg / 1000, mirror: null }, { rotDeg: item.rotMillideg / 1000, mirror: null }, { Value: item.net });
    if (drawn.fields_autoplaced !== undefined) ps.fields_autoplaced = drawn.fields_autoplaced;
  }
  return { ...EMPTY, power_symbols: [ps], lib_symbols: lib ? { [item.libId]: lib } : {} };
}

/** Two carried items that draw alike: what the server drew for one is what it would draw for the other. */
export function sameItem(a: CarriedItem, b: CarriedItem): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** The carried item as the page draws it at `PREVIEW_AT`, in `GET /api/schematic`'s shapes, with what the server's drawing adds (see the file's doc). */
export function carriedPreview(item: CarriedItem, ctx: CarriedPreviewContext): SchPastePreview {
  const server = ctx.server ?? null;
  switch (item.kind) {
    case "label": {
      const label: SchematicLabel = {
        id: CARRIED_IDS.label,
        net: item.text,
        at: AT,
        scope: item.scope,
        shape: item.scope === "local" ? null : item.shape,
        // "From the wire": none, so it is read off the wire it ends on where it is drawn
        spin: item.spin || null,
        // none is the IR's 50 mil (a new label starts at the setup's size: the dialog sends it)
        size_nm: item.sizeNm ?? 1_270_000,
        bold: item.bold,
        italic: item.italic,
        auto_rotate: item.scope !== "local" && item.autoRotate,
      };
      // a global label's Sheet References, as the server placed them for this very label
      const drawn = server && server.item.kind === "label" && sameItem(server.item, item) ? server.preview.labels[0] : undefined;
      if (drawn?.fields) label.fields = drawn.fields;
      return { ...EMPTY, labels: [label] };
    }
    case "text": {
      const text: SchematicText = { id: CARRIED_IDS.text, content: item.content, at: AT, angle: item.vertical ? 90 : 0, size_nm: item.sizeNm, vertical: item.vertical, h: item.h, v: item.v };
      // `${..}` as the server resolves it, for this very text (a turn does not change what it shows)
      const drawn = server && server.item.kind === "text" && server.item.content === item.content ? server.preview.texts[0] : undefined;
      if (drawn?.shown !== undefined) text.shown = drawn.shown;
      return { ...EMPTY, texts: [text] };
    }
    case "directive": {
      const g: SchGraphic = {
        id: CARRIED_IDS.directive,
        shape: { type: "directive", at: { x: AT[0], y: AT[1] }, orientation: item.orientation, shape: item.shape, pin_length_nm: item.pinLengthNm, netclass: item.netclass, component_class: item.componentClass },
      };
      return { ...EMPTY, graphics: [g] };
    }
    case "power":
      return powerPreview(item, ctx);
    case "symbol":
      return symbolPreview(item, ctx);
  }
}

/** Does the page need the server's drawing of `item` (see the file's doc): a symbol's or power symbol's fields, a global label's Sheet References, a text's variables. */
export function needsServerDrawing(item: CarriedItem): boolean {
  switch (item.kind) {
    case "symbol":
    case "power":
      return true;
    case "label":
      return item.scope === "global";
    case "text":
      return /[$@]\{/.test(item.content);
    case "directive":
      return false;
  }
}
