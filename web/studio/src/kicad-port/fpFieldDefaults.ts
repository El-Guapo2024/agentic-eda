// Preferences > Footprint Editor > Footprint Defaults and > User Layer Names (UI-LEFT-2 FF7), and what the Footprint Editor does with them, KiCad 8303b2ad:
//
//   * Footprint Defaults (`PANEL_FP_EDITOR_FIELD_DEFAULTS`, pcbnew/dialogs/panel_fp_editor_field_defaults.cpp): "Default Field Properties for New Footprints" -- the
//     Reference designator and Value rows (Value, Show, Layer) -- and "Default Text Items for New Footprints" (Text Items, Layer; Add / Delete), kept as one list,
//     `FOOTPRINT_EDITOR_SETTINGS::m_DesignSettings.m_DefaultFPTextItems` (`design_settings.default_footprint_text_items`: `{ "REF**", true, F.SilkS }`,
//     `{ "", true, F.Fab }`, `{ "${REFERENCE}", true, F.Fab }` by default). `PCB_BASE_FRAME::CreateNewFootprint` (footprint_libraries_utils.cpp), which New Footprint
//     runs, makes the new footprint's Reference, Value and texts from it: each on its layer, in its layer class's text size, thickness, italic and keep-upright
//     (`settings.GetTextSize( layer )` ...: the Graphics Defaults page), one under the other from half a text height above the origin; an empty Reference or Value is
//     the footprint's name.
//   * User Layer Names (`PANEL_FP_USER_LAYER_NAMES`, panel_fp_user_layer_names.cpp): "User layers:" (0 .. 9, `user_layer_count`, 4 by default) and a grid of a user
//     layer and its name (`m_UserLayerNames`, `design_settings.default_footprint_layer_names`, by the layer's canonical name). The editor enables the first so many
//     user layers and every named one (`FOOTPRINT_EDIT_FRAME::UpdateUserInterface`) and shows a named one by its name (`SetLayerName`, `APPEARANCE_CONTROLS::
//     rebuildLayers`' `m_UserLayerNames[ canonicalName ]`).
//
// Pure: the pages (components/FpDefaultsPages.tsx) and the Footprint Editor's New Footprint (state/footprintEditorStore.tsx) call these.
import type { CmdText, FieldLayoutCmd, TextGraphicsDefaults } from "../api/types";
import type { TextLook } from "../api/fpEditorTypes";
import { textStyleOf } from "./layerDefaults";
import { TECH_LAYER_NAMES } from "./layerPresets";
import { userLayerName, userLayerNumber } from "./boardLayers";

/** `TEXT_ITEM_INFO`: a text, whether it is shown, its layer (a board file layer name). */
export interface FpTextItem {
  text: string;
  visible: boolean;
  layer: string;
}

/** `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS`' `m_DefaultFPTextItems` (and the param's default): the reference, the value, then "freebies". */
export const DEFAULT_FP_TEXT_ITEMS: readonly FpTextItem[] = [
  { text: "REF**", visible: true, layer: "F.SilkS" },
  { text: "", visible: true, layer: "F.Fab" },
  { text: "${REFERENCE}", visible: true, layer: "F.Fab" },
];

/** The layers a text item's Layer cell offers (`GRID_CELL_LAYER_SELECTOR( nullptr, {} )`: every layer, none forbidden): the outer copper, the technical and user-mask layers, User.1 .. User.45. */
export const TEXT_ITEM_LAYERS: readonly string[] = ["F.Cu", "B.Cu", ...TECH_LAYER_NAMES, ...Array.from({ length: 45 }, (_, i) => userLayerName(i + 1))];

/** The setting as stored: anything malformed is dropped (a layer that is no layer is F.SilkS, as the param's reader has it: `candidateLayer >= 0 ? .. : F_SilkS`); `null` is KiCad's defaults. */
export function sanitizeFpTextItems(raw: unknown): FpTextItem[] | null {
  if (!Array.isArray(raw)) return null;
  const out: FpTextItem[] = [];
  for (const e of raw as Array<Record<string, unknown>>) {
    if (!e || typeof e !== "object") continue;
    const layer = typeof e.layer === "string" && TEXT_ITEM_LAYERS.includes(e.layer) ? e.layer : "F.SilkS";
    out.push({ text: typeof e.text === "string" ? e.text : "", visible: typeof e.visible === "boolean" ? e.visible : true, layer });
  }
  return out;
}

/** The setting in force: the person's list, or KiCad's defaults. */
export function fpTextItemsOf(stored: readonly FpTextItem[] | null | undefined): FpTextItem[] {
  return (stored ?? DEFAULT_FP_TEXT_ITEMS).map((i) => ({ ...i }));
}

// ---------------------------------------------------------------------------------------------------------------------------------- the Footprint Defaults page

/** The page's two grids: the two field rows (`TEXT_ITEMS_GRID_TABLE( true )`: Value, Show, Layer) and the text item rows (`( false )`: Text Items, Layer). */
export interface FieldDefaultsPage {
  fields: [FpTextItem, FpTextItem];
  texts: Array<{ text: string; layer: string }>;
}

/** `TEXT_ITEMS_GRID_TABLE::AppendRows`' new row: `( wxT( "" ), true, F_SilkS )`. */
const blankRow = (): FpTextItem => ({ text: "", visible: true, layer: "F.SilkS" });

/** `loadFPSettings`: two field rows made blank, then filled from the first two items there are; the text items from the third on. */
export function loadFieldDefaults(items: readonly FpTextItem[]): FieldDefaultsPage {
  const fields: [FpTextItem, FpTextItem] = [blankRow(), blankRow()];
  for (let i = 0; i < Math.min(2, items.length); i++) fields[i] = { ...items[i]! };
  return { fields, texts: items.slice(2).map((t) => ({ text: t.text, layer: t.layer })) };
}

/** `TransferDataFromWindow`: the two field rows, then each text item, shown (`cfg.m_DefaultFPTextItems.emplace_back( text, true, layer )`). */
export function fieldDefaultsItems(page: FieldDefaultsPage): FpTextItem[] {
  return [{ ...page.fields[0] }, { ...page.fields[1] }, ...page.texts.map((t) => ({ text: t.text, visible: true, layer: t.layer }))];
}

/** `OnAddTextItem`: a row at the end, on the layer of the row before it (F.SilkS for the first). */
export function addTextItemRow(page: FieldDefaultsPage): FieldDefaultsPage {
  const layer = page.texts.length > 0 ? page.texts[page.texts.length - 1]!.layer : "F.SilkS";
  return { ...page, texts: [...page.texts, { text: "", layer }] };
}

/** `OnDeleteTextItem` (`GRID_TRICKS` `OnDeleteRows`): the rows selected go. */
export function deleteTextItemRows(page: FieldDefaultsPage, rows: ReadonlySet<number>): FieldDefaultsPage {
  return { ...page, texts: page.texts.filter((_, i) => !rows.has(i)) };
}

/** `ResetPanel`: a fresh `FOOTPRINT_EDITOR_SETTINGS`' list, KiCad's defaults. */
export function resetFieldDefaults(): FieldDefaultsPage {
  return loadFieldDefaults(DEFAULT_FP_TEXT_ITEMS);
}

// ---------------------------------------------------------------------------------------------------------------------------------- New Footprint

/** What `CreateNewFootprint` gives a new footprint from the text items: the Reference's and the Value's text and layout, and the other texts. */
export interface NewFootprintTexts {
  /** The Reference's text and layout (`text` is what it shows: `REF**` by default; an empty one is the footprint's name). */
  reference: { text: string; layout: FieldLayoutCmd };
  value: { text: string; layout: FieldLayoutCmd };
  texts: CmdText[];
  /** Each of `texts`' looks beyond `CmdText` (`fp_item set_text`'s `look`): its layer class's glyph width, italic and keep-upright (UI-LEFT-3 F3-6). */
  looks: TextLook[];
}

/** A text's look as the backend makes one with none of its own (`TextLook`'s defaults: upright, the width its height). */
export const PLAIN_TEXT_LOOK: TextLook = { bold: false, italic: false, valign: 0, width_nm: null, knockout: false, keep_upright: true };

/**
 * `PCB_BASE_FRAME::CreateNewFootprint`: the Reference from the first item, the Value from the second, a text for each other item; `default_pos` starts at the
 * origin, a text goes half its layer's text height further down (the Reference half a height up), and the next one starts a whole height below it; each text takes
 * its layer class's size, thickness, italic and keep-upright (`RunOnChildren`: `SetTextThickness( GetTextThickness( layer ) )` ...). An empty Reference or Value
 * shows the footprint's name (`if( footprint->GetReference().IsEmpty() ) footprint->SetReference( aFootprintName )`). A text item's visibility is the item's for the
 * two fields; the other texts are made shown (`PCB_TEXT`'s default). A text's glyph width, italic and keep-upright are its look (`looks`, UI-LEFT-3 F3-6; they
 * were the fields' only). Nothing is mirrored: `CreateNewFootprint` only sets each text's layer.
 */
export function newFootprintTexts(items: readonly FpTextItem[], graphics: TextGraphicsDefaults | null, name: string): NewFootprintTexts {
  const all = items.length >= 2 ? items : [...items, ...DEFAULT_FP_TEXT_ITEMS.slice(items.length, 2)];
  let y = 0;
  const layoutOf = (item: FpTextItem, at: number): FieldLayoutCmd => {
    const s = textStyleOf(graphics ?? undefined, item.layer);
    return { at: { x: 0, y: at }, size: [s.widthNm, s.heightNm], thickness: s.thicknessNm, layer: item.layer, visible: item.visible, italic: s.italic, keep_upright: s.upright };
  };
  const height = (layer: string) => textStyleOf(graphics ?? undefined, layer).heightNm;
  // the Reference: `default_pos.y -= GetTextSize( layer ).y / 2`, then a whole height on
  const ref = all[0]!;
  y -= height(ref.layer) / 2;
  const reference = { text: ref.text === "" ? name : ref.text, layout: layoutOf(ref, y) };
  y += height(ref.layer);
  const val = all[1]!;
  y += height(val.layer) / 2;
  const value = { text: val.text === "" ? name : val.text, layout: layoutOf(val, y) };
  y += height(val.layer);
  const texts: CmdText[] = [];
  const looks: TextLook[] = [];
  for (const item of all.slice(2)) {
    y += height(item.layer) / 2;
    const s = textStyleOf(graphics ?? undefined, item.layer);
    texts.push({ content: item.text, at: { x: 0, y }, angle: 0, layer: item.layer, size_nm: s.heightNm, stroke_width: s.thicknessNm, justify: "center", mirror: false });
    // `SetTextSize( settings.GetTextSize( layer ) )`, `SetItalic( GetTextItalic( layer ) )`, `SetKeepUpright( GetTextUpright( layer ) )`
    looks.push({ ...PLAIN_TEXT_LOOK, width_nm: s.widthNm === s.heightNm ? null : s.widthNm, italic: s.italic, keep_upright: s.upright });
    y += height(item.layer);
  }
  return { reference, value, texts, looks };
}

/** True when the new footprint the backend makes (KiCad's default items in KiCad's factory sizes, crates/model fp_edit.rs) is already what the settings ask for. */
export function isFactoryNewFootprint(items: readonly FpTextItem[], graphics: TextGraphicsDefaults | null, name: string): boolean {
  return JSON.stringify(newFootprintTexts(items, graphics, name)) === JSON.stringify(newFootprintTexts(DEFAULT_FP_TEXT_ITEMS, null, name));
}

// ---------------------------------------------------------------------------------------------------------------------------------- User Layer Names

/** "User layers:" offers 0 .. 9 (`m_choiceUserLayers`); `user_layer_count` is 4 by default. */
export const USER_LAYER_COUNT_CHOICES: readonly number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
export const DEFAULT_USER_LAYER_COUNT = 4;

/** The layers a row's Layer cell offers: every layer but the copper and technical ones, Edge.Cuts and Margin (`forbiddenLayers`) -- the user-mask layers and User.1 .. User.45. */
export const USER_NAME_LAYERS: readonly string[] = ["Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", ...Array.from({ length: 45 }, (_, i) => userLayerName(i + 1))];

/** `IsUserLayer`: Dwgs.User, Cmts.User, Eco1.User, Eco2.User and the User.n layers. */
export function isUserLayer(layer: string): boolean {
  return USER_NAME_LAYERS.includes(layer);
}

export interface LayerNameRow {
  layer: string;
  name: string;
}

/** `loadFPSettings`: a row for each named user layer, in the map's order (`std::map` by the canonical name, a byte compare). */
export function loadLayerNames(names: Readonly<Record<string, string>>): LayerNameRow[] {
  return Object.keys(names)
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))
    .filter(isUserLayer)
    .map((layer) => ({ layer, name: names[layer]! }));
}

/** `LAYER_NAMES_GRID_TABLE::AppendRows`: a row on the first User.n no row has (`layer = layer + 2` from User_1); none left: nothing added (`return false`). */
export function addLayerNameRow(rows: readonly LayerNameRow[]): LayerNameRow[] {
  const used = new Set(rows.map((r) => r.layer));
  for (let n = 1; n <= 45; n++) {
    const layer = userLayerName(n);
    if (!used.has(layer)) return [...rows, { layer, name: "" }];
  }
  return [...rows];
}

/** `getNextAvailableLayer`: the first of User_1 .. User_44 no row has (`for( int ii = User_1; ii < User_45; ++ii )`); null when there is none. */
export function nextAvailableLayer(rows: readonly LayerNameRow[]): string | null {
  const used = new Set(rows.map((r) => r.layer));
  // `++ii` also walks the copper ids between the User ids; those are never a row's layer, so the first free User.n below User.45 is the answer
  for (let n = 1; n < 45; n++) if (!used.has(userLayerName(n))) return userLayerName(n);
  return null;
}

/**
 * `onLayerChange` for row `row`: a layer another row already has gives the row the next free layer instead; then a name another row has is "Layer name %s already
 * in use." (the dialog's error, `SetError`, which does not stop OK).
 */
export function changeLayerNameRow(rows: readonly LayerNameRow[], row: number, patch: Partial<LayerNameRow>): { rows: LayerNameRow[]; error: string | null } {
  const out = rows.map((r, i) => (i === row ? { ...r, ...patch } : { ...r }));
  const edited = out[row];
  if (!edited) return { rows: out, error: null };
  if (patch.layer !== undefined && out.some((r, i) => i !== row && r.layer === edited.layer)) {
    const next = nextAvailableLayer(out.filter((_, i) => i !== row));
    // (KiCad's `SetValueAsLong( row, 0, -1 )` when none is free leaves a row of no layer, which OK drops; here the row keeps its layer and OK keeps the first)
    if (next) edited.layer = next;
    return { rows: out, error: null };
  }
  const clash = out.find((r, i) => i !== row && r.name === edited.name);
  return { rows: out, error: clash ? `Layer name ${edited.name} already in use.` : null };
}

/** `TransferDataFromWindow`: the rows of a user layer with a name, the first row of a layer winning (`emplace`). */
export function layerNamesOf(rows: readonly LayerNameRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) if (isUserLayer(r.layer) && r.name !== "" && !(r.layer in out)) out[r.layer] = r.name;
  return out;
}

/** The stored names, cleaned: user layers with a name only. */
export function sanitizeUserLayerNames(raw: unknown): Record<string, string> {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(raw as Record<string, unknown>)) if (isUserLayer(k) && typeof v === "string" && v !== "") out[k] = v;
  return out;
}

/**
 * The User.n layers the Footprint Editor enables (`UpdateUserInterface`): the first `count` (`LSET::UserDefinedLayersMask( userLayerCount )`) and every User.n that has a
 * name (`m_UserLayerNames.contains( LSET::Name( user ) )` over `UserDefinedLayersMask()`), by number.
 */
export function enabledUserLayers(count: number, names: Readonly<Record<string, string>>): number[] {
  const out = new Set<number>();
  for (let n = 1; n <= count; n++) out.add(n);
  for (const k of Object.keys(names)) {
    const n = userLayerNumber(k);
    if (n !== null) out.add(n);
  }
  return [...out].sort((a, b) => a - b);
}
