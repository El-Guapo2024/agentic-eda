// The Footprint Editor's Appearance manager (UI-LEFT F2) as pure functions: the layers its Layers tab lists and what it calls them, the rows its Objects tab keeps,
// the inactive-layer modes, what the layer list's right-click menu does to the editor's visibility, and which items a click may still pick once some are hidden.
// components/footprint/FootprintAppearancePanel.tsx renders it on the editor's own store (state/footprintEditorStore.tsx: `layerVisible`, `layerOpacity`, `activeLayer`,
// `highContrast` with `contrastHidden`, `gridVisible`); the painter (components/footprint/footprintPainter.ts, fpItemsPaint.ts) draws by it and the canvas's hit test
// (kicad-port/fpItems.ts `fpHitTest`) picks by it.
//
// The keys are the board's (kicad-port/appearance.ts): a layer is off under its KiCad name or its state key (`layerStateKey`, `layerIsVisible` reads either), an
// object row under `obj:<id>`, an object's opacity under the same `obj:<id>` in `layerOpacity`. Layers here are KiCad names ("F.SilkS"), as the editor's `activeLayer` is.
//
// Ported from (KiCad 8303b2ad unless said):
//   pcbnew/widgets/appearance_controls.cpp  `APPEARANCE_CONTROLS` built with `aFpEditorMode` (`m_isFpEditor`): rebuildLayers (the copper layers front to back by
//                                           `board->GetLayerName`, then `non_cu_seq` by the editor's own user layer names or `GetStandardLayerName`, each with its
//                                           tooltip), rebuildObjects with `s_allowedInFpEditor` (the set of KiCad 25ec2335, which adds LAYER_VIA_STITCHING to the eleven
//                                           of 8303b2ad; the spacers are not in the set, so the footprint editor's list has none), the Nets page removed
//                                           (`m_notebook->RemovePage( 2 )`), rebuildLayerContextMenu / OnLayerContextMenu, onObjectVisibilityChanged (Footprint Text drags
//                                           References and Values; Grid is the frame's grid switch), onObjectOpacitySlider, UpdateDisplayOptions (the three contrast modes)
//   pcbnew/footprint_edit_frame.cpp         updateEnabledLayers (the editor's board has every technical and `UserMask` layer, F.Cu, In1.Cu named "Inner layers" and B.Cu for
//                                           a footprint's default stackup `EXPAND_INNER_LAYERS`, the user layers the footprint's items are on, and the first
//                                           `user_layer_count` user layers)
//   pcbnew/footprint_editor_settings.cpp    `design_settings.user_layer_count`: 4
//   pcbnew/footprint.cpp                    FOOTPRINT's default stackup: `{ F_Cu, In1_Cu, B_Cu }`, `EXPAND_INNER_LAYERS` (so `GetCopperLayerCount()` is 3 and the menu offers
//                                           "Show Only Inner Layers")
//   include/pcb_display_options.h           every opacity 1.0 to begin with (the footprint editor has no project, so none of the board's 0.6)
//   pcbnew/tools/pcb_selection_tool.cpp     Selectable (what a click may pick: an item on a visible layer, with its object shown and not at opacity 0, and with high contrast
//                                           only an item on the active layer)
//   pcbnew/pcb_painter.cpp                  GetColor's per-type opacity (a filled shape takes Filled Shapes', a copper shape the Tracks')
//   pcbnew/pcb_text.cpp, pcb_field.cpp      ViewGetLOD (a footprint's text `${REFERENCE}` follows References and `${VALUE}` Values; every text and field Footprint Text)
//   pcbnew/pad.cpp                          ViewGetLOD (a pad shows while one of its layers does), IsOnLayer
//
// Differences: the user layers the footprint's items use are read from the footprint as it is now (`updateEnabledLayers` reads the copy taken when it was loaded, so a
// layer first drawn on in this session would get its row on the next load). KiCad's footprint editor asks its board whether the Pads, Zones and Points rows are on
// (`board()->IsElementVisible`), and a board with no project answers yes, so there an item hidden by those rows can still be clicked; here what is not drawn is not
// picked, as in the board editor.
//
// Pure: no store, no DOM. Unit-tested in fpAppearance.test.ts.
import type { LibraryFootprint } from "../api/types";
import type { LayerPreset } from "./appearance";
import { OBJECT_IDS, OBJECT_ROWS, objectKey, objectOn, withObjectVisible, type ContrastMode, type ObjectId, type ObjectRow, type Opacity } from "./appearance";
import { hideAllButActive, isCopper, layerIsVisible, layerStateKey, menuPreset, TECH_LAYER_NAMES, firstLayer, type LayerGroupOp, type MenuPresetKind, type PresetView } from "./layerPresets";
import { isCopperLayerName, MAX_USER_LAYERS, standardLayerName, userLayerName, userLayerNumber } from "./boardLayers";
import { zoneLayers } from "./zoneLayers";

// ------------------------------------------------------------------------------------------------------------------------------------ layers

/** The copper layers of the editor's board for a footprint's default stackup (`EXPAND_INNER_LAYERS`: `enabledLayers |= LSET{ F_Cu, In1_Cu, B_Cu }`), front to back. */
export const FP_COPPER_LAYERS: readonly string[] = ["F.Cu", "In1.Cu", "B.Cu"];

/** `board.SetLayerName( In1_Cu, _( "Inner layers" ) )`: the one inner layer stands for all of them. */
export const FP_INNER_LAYERS_LABEL = "Inner layers";

/** `FOOTPRINT_EDITOR_SETTINGS`' `design_settings.user_layer_count` (4): the user layers the editor's board has whatever the footprint uses. */
export const FP_USER_LAYER_COUNT = 4;

/** The fields of a footprint that say which layers its items are on. */
export type FpLayerSource = Pick<LibraryFootprint, "pads" | "graphics" | "texts" | "fields" | "reference" | "value" | "datasheet" | "description_field" | "display">;

/** Every layer an item of the footprint is on (pads, graphics, texts, fields with a layout of their own, and the items `display` holds). */
export function fpUsedLayers(fp: FpLayerSource | null | undefined): string[] {
  if (!fp) return [];
  const out = new Set<string>();
  for (const p of fp.pads ?? []) for (const l of p.layers) out.add(l);
  for (const g of fp.graphics ?? []) out.add(g.layer);
  for (const t of fp.texts ?? []) out.add(t.layer);
  for (const f of [fp.reference, fp.value, fp.datasheet, fp.description_field]) if (f?.layer) out.add(f.layer);
  for (const f of fp.fields ?? []) if (f.layout?.layer) out.add(f.layout.layer);
  const d = fp.display;
  if (d) {
    for (const z of d.zones ?? []) for (const l of zoneLayers(z)) out.add(l);
    for (const list of [d.dimensions, d.text_boxes, d.images, d.points, d.barcodes, d.tables]) for (const x of list ?? []) if (x.layer) out.add(x.layer);
  }
  return [...out];
}

/**
 * The Layers tab's rows (`rebuildLayers` on the editor's board, `updateEnabledLayers`): the copper layers front to back, then the technical and user-mask layers in
 * `non_cu_seq`'s order, then the user layers -- the first `userLayerCount` (Preferences > Footprint Editor > User Layer Names' "User layers:") and every other one
 * `used` names (an item's layer, or a layer that has a name there: `UpdateUserInterface`) -- by number.
 */
export function fpPanelLayers(used: Iterable<string> = [], userLayerCount = FP_USER_LAYER_COUNT): string[] {
  const users = new Set<number>();
  for (let n = 1; n <= userLayerCount; n++) users.add(n);
  for (const l of used) {
    const n = userLayerNumber(l);
    if (n !== null) users.add(n);
  }
  return [...FP_COPPER_LAYERS, ...TECH_LAYER_NAMES, ...[...users].sort((a, b) => a - b).map(userLayerName)];
}

/**
 * A row's name: a copper layer's `board->GetLayerName` (the editor's board names only In1.Cu, "Inner layers"), a user layer the Footprint Editor names
 * (`m_UserLayerNames[ canonicalName ]`, Preferences > Footprint Editor > User Layer Names, UI-LEFT-2 FF7), any other layer's `GetStandardLayerName` ("F.Silkscreen",
 * "User.Drawings" ...).
 */
export function fpLayerLabel(name: string, userLayerNames: Readonly<Record<string, string>> = {}): string {
  if (name === "In1.Cu") return FP_INNER_LAYERS_LABEL;
  if (userLayerNames[name]) return userLayerNames[name]!;
  return isCopperLayerName(name) ? name : standardLayerName(name);
}

/** One entry of a layer choice: the layer's canonical name (what an item keeps) and the name it is shown by. */
export interface FpLayerChoice {
  value: string;
  label: string;
}

/**
 * The layers the Footprint Editor's layer choices offer, each by its name (UI-LEFT-3 F3-1): the toolbar's layer box (`PCB_LAYER_BOX_SELECTOR::Resync` with the frame's board:
 * `GetEnabledLayers()` in `UIOrder()`, named by `getLayerName` -> `BOARD::GetLayerName`), the dialogs' (`m_LayerSelectionCtrl->SetBoardFrame( frame ); Resync()`) and the
 * Properties pane's (`PCB_PROPERTIES_PANEL::updateLists`: `GetEnabledLayers().UIOrder()`, shown by `GetLayerName`). The enabled layers are the Appearance panel's
 * (`updateEnabledLayers`: [`fpPanelLayers`]), so a user layer appears by the name Preferences > Footprint Editor > User Layer Names gives it (`LoadSettings`' `SetLayerName`).
 */
export function fpLayerChoices(fp: FpLayerSource | null | undefined, userLayerNames: Readonly<Record<string, string>> = {}, userLayerCount = FP_USER_LAYER_COUNT): FpLayerChoice[] {
  return fpPanelLayers([...fpUsedLayers(fp), ...Object.keys(userLayerNames)], userLayerCount).map((value) => ({ value, label: fpLayerLabel(value, userLayerNames) }));
}

/** Every layer of a board in `UIOrder()` (`LSET::AllCuMask() | LSET::AllNonCuMask()`): F.Cu, In1.Cu .. In30.Cu, B.Cu, the technical layers, User.1 .. User.45. */
export function fpAllLayers(): string[] {
  const inner = Array.from({ length: 30 }, (_, i) => `In${i + 1}.Cu`);
  return ["F.Cu", ...inner, "B.Cu", ...TECH_LAYER_NAMES, ...Array.from({ length: MAX_USER_LAYERS }, (_, i) => userLayerName(i + 1))];
}

/**
 * A text dialog's layer choice when the text is on a layer the editor's board has not enabled (`DIALOG_TEXT_PROPERTIES`: `if( !m_frame->GetBoard()->IsLayerEnabled(
 * m_item->GetLayer() ) ) m_LayerSelectionCtrl->ShowNonActivatedLayers( true )`): every layer, one not enabled shown by its standard name with " (not activated)"
 * (`Resync`'s `layerstatus`); otherwise the enabled ones ([`fpLayerChoices`]).
 */
export function fpTextLayerChoices(layer: string, fp: FpLayerSource | null | undefined, userLayerNames: Readonly<Record<string, string>> = {}, userLayerCount = FP_USER_LAYER_COUNT): FpLayerChoice[] {
  const enabled = fpLayerChoices(fp, userLayerNames, userLayerCount);
  if (enabled.some((c) => c.value === layer)) return enabled;
  const on = new Map(enabled.map((c) => [c.value, c.label]));
  return fpAllLayers().map((value) => ({ value, label: on.get(value) ?? `${fpLayerLabel(value)} (not activated)` }));
}

/** `non_cu_seq`'s tooltips. */
const NON_CU_TOOLTIP: Readonly<Record<string, string>> = {
  "F.Adhes": "Adhesive on board's front",
  "B.Adhes": "Adhesive on board's back",
  "F.Paste": "Solder paste on board's front",
  "B.Paste": "Solder paste on board's back",
  "F.SilkS": "Silkscreen on board's front",
  "B.SilkS": "Silkscreen on board's back",
  "F.Mask": "Solder mask on board's front",
  "B.Mask": "Solder mask on board's back",
  "Dwgs.User": "Explanatory drawings",
  "Cmts.User": "Explanatory comments",
  "Eco1.User": "User defined meaning",
  "Eco2.User": "User defined meaning",
  "Edge.Cuts": "Board's perimeter definition",
  Margin: "Board's edge setback outline",
  "F.CrtYd": "Footprint courtyards on board's front",
  "B.CrtYd": "Footprint courtyards on board's back",
  "F.Fab": "Footprint assembly on board's front",
  "B.Fab": "Footprint assembly on board's back",
};

/** The tooltip of a row's name (`setting->tooltip`): "Front copper layer", "Inner copper layer", `non_cu_seq`'s text, "User defined layer N". */
export function fpLayerTooltip(name: string): string {
  if (name === "F.Cu") return "Front copper layer";
  if (name === "B.Cu") return "Back copper layer";
  if (isCopperLayerName(name)) return "Inner copper layer";
  const n = userLayerNumber(name);
  if (n !== null) return `User defined layer ${n}`;
  return NON_CU_TOOLTIP[name] ?? "";
}

/** The `layerVisible` entries that show or hide `names`, each under its name and its state key (the painter and the hit test accept either, and an old entry under one must not win). */
export function fpLayersPatch(names: Iterable<string>, on: boolean): Record<string, boolean> {
  const out: Record<string, boolean> = {};
  for (const n of names) {
    out[n] = on;
    out[layerStateKey(n)] = on;
  }
  return out;
}

/** The `layerVisible` entries that leave exactly `visible` of `universe` on (`setVisibleLayers( aLayers )`: every layer the panel lists is set). */
export function fpVisibleLayersPatch(universe: readonly string[], visible: ReadonlySet<string>): Record<string, boolean> {
  return {
    ...fpLayersPatch(
      universe.filter((l) => !visible.has(l)),
      false
    ),
    ...fpLayersPatch(
      universe.filter((l) => visible.has(l)),
      true
    ),
  };
}

// ------------------------------------------------------------------------------------------------------------------------------------ the layer menu

/** "Show Only Inner Layers" is offered when the board has more than two copper layers (`GetCopperLayerCount() > 2`): the editor's board has three. */
export const FP_SHOWS_INNER_LAYERS_ENTRY = FP_COPPER_LAYERS.length > 2;

/** The entries of the layer list's right-click menu (`rebuildLayerContextMenu`), by what they do. */
export type FpLayerMenuCommand = LayerGroupOp | "hide_all_but_active" | MenuPresetKind;

/** `presetAllCopper.layers`: `LSET( LSET::AllCuMask() ).set( Edge_Cuts )` -- the copper / non-copper entries move the board outline with the copper. */
const inAllCopper = (l: string): boolean => isCopper(l) || l === "Edge.Cuts";

/**
 * `OnLayerContextMenu` on the editor's layers: the four copper / non-copper entries change the visible set (the two that hide move the active layer to the first one left on
 * when theirs went: `SetActiveLayer( *visible.Seq().begin() )`), "Hide All Layers But Active" and the preset entries apply a preset that keeps the objects as they are
 * (kicad-port/layerPresets.ts `hideAllButActive`, `menuPreset`). Returns the `layerVisible` entries for every listed layer and the active layer after it.
 */
export function fpLayerMenu(universe: readonly string[], layerVisible: Readonly<Record<string, boolean>>, activeLayer: string, cmd: FpLayerMenuCommand): { visible: Record<string, boolean>; activeLayer: string } {
  const visible = new Set(universe.filter((l) => layerIsVisible(layerVisible, l)));
  const moveActive = (): string => (!visible.has(activeLayer) && visible.size > 0 ? (firstLayer(visible) ?? activeLayer) : activeLayer);
  switch (cmd) {
    case "show_copper":
      for (const l of universe) if (inAllCopper(l)) visible.add(l);
      return { visible: fpVisibleLayersPatch(universe, visible), activeLayer };
    case "hide_copper":
      for (const l of universe) if (inAllCopper(l)) visible.delete(l);
      return { visible: fpVisibleLayersPatch(universe, visible), activeLayer: moveActive() };
    case "hide_non_copper":
      for (const l of universe) if (!inAllCopper(l)) visible.delete(l);
      return { visible: fpVisibleLayersPatch(universe, visible), activeLayer: moveActive() };
    case "show_non_copper":
      for (const l of universe) if (!inAllCopper(l)) visible.add(l);
      return { visible: fpVisibleLayersPatch(universe, visible), activeLayer };
    default: {
      // a preset: the objects and the flip are kept (`preset.renderLayers = getVisibleObjects()`), so only the layers and the active layer matter here
      const view: PresetView = { universe, visibleLayers: visible, objects: [], activeLayer, flipBoard: false, activePreset: "", lastBuiltinObjects: null, boardLayers: new Set(universe) };
      const next =
        cmd === "hide_all_but_active"
          ? hideAllButActive(view)
          : menuPreset(
              view,
              cmd,
              universe.filter((l) => isCopper(l)),
              universe.filter((l) => !isCopper(l))
            );
      return { visible: fpVisibleLayersPatch(universe, new Set(next.visibleLayers)), activeLayer: next.activeLayer ?? activeLayer };
    }
  }
}

// ------------------------------------------------------------------------------------------------------------------------------------ objects

/** `s_allowedInFpEditor` (KiCad 25ec2335): the objects the footprint editor's Objects tab lists. */
export const FP_EDITOR_OBJECTS: readonly ObjectId[] = ["tracks", "vias", "pads", "zones", "shapes", "footprint_values", "footprint_references", "footprint_text", "bitmaps", "grid", "ly_points", "via_stitching"];

/** The Objects tab's rows: `s_objectSettings` in its order, those of `s_allowedInFpEditor` only (no spacer: a spacer's id, -1, is not in the set). */
export const FP_OBJECT_ROWS: readonly (ObjectRow & { id: ObjectId })[] = OBJECT_ROWS.filter((r): r is ObjectRow & { id: ObjectId } => r.id !== null && FP_EDITOR_OBJECTS.includes(r.id));

/** An object's opacity (`obj:<id>` of `layerOpacity`), 1 unless set: `PCB_DISPLAY_OPTIONS` starts every opacity at 1.0. */
export function fpObjectOpacity(layerOpacity: Readonly<Record<string, number>> | undefined, id: ObjectId): number {
  return layerOpacity?.[objectKey(id)] ?? 1;
}

/** The six opacities of `PCB_DISPLAY_OPTIONS` (the painter's `Opacity`), from the editor's `layerOpacity`. */
export function fpOpacities(layerOpacity: Readonly<Record<string, number>> | undefined): Opacity {
  const o = (id: ObjectId) => fpObjectOpacity(layerOpacity, id);
  return { tracks: o("tracks"), vias: o("vias"), pads: o("pads"), zones: o("zones"), images: o("bitmaps"), shapes: o("shapes") };
}

/** A row's eye: the object's `obj:` switch; the Grid row is the frame's grid (`SetObjectVisible( LAYER_GRID, m_frame->IsGridVisible() )`). */
export function fpObjectChecked(s: { layerVisible: Readonly<Record<string, boolean>>; gridVisible: boolean }, id: ObjectId): boolean {
  return id === "grid" ? s.gridVisible && objectOn(s.layerVisible, "grid") : objectOn(s.layerVisible, id);
}

/**
 * `onObjectVisibilityChanged` for a row's eye: the `obj:` entries that change. Footprint Text turns References and Values with it, and References or Values turned on
 * put Footprint Text back on (kicad-port/appearance.ts `withObjectVisible`).
 */
export function fpObjectPatch(layerVisible: Readonly<Record<string, boolean>>, id: ObjectId, on: boolean): Record<string, boolean> {
  const current = Object.fromEntries(OBJECT_IDS.map((o) => [o, objectOn(layerVisible, o)])) as Record<ObjectId, boolean>;
  const next = withObjectVisible(current, id, on);
  const patch: Record<string, boolean> = { [objectKey(id)]: on };
  for (const o of OBJECT_IDS) if (next[o] !== current[o]) patch[objectKey(o)] = next[o];
  return patch;
}

// ------------------------------------------------------------------------------------------------------------------------------------ contrast

/** `m_ContrastModeDisplay` from the editor's two switches: not high contrast is NORMAL, high contrast DIMMED or, with `contrastHidden`, HIDDEN. */
export function fpContrastMode(highContrast: boolean, contrastHidden: boolean): ContrastMode {
  return !highContrast ? "normal" : contrastHidden ? "hidden" : "dimmed";
}

// ------------------------------------------------------------------------------------------------------------------------------------ drawing and picking

/** The layers a library pad's layer name stands for on the editor's board: `*.Cu` every copper layer (F.Cu, In1.Cu, B.Cu), `*.Mask` both masks ..., `F&B.Cu` the two outer ones. */
function padLayerSet(l: string): string[] {
  if (l === "*.Cu") return [...FP_COPPER_LAYERS];
  if (l === "F&B.Cu") return ["F.Cu", "B.Cu"];
  if (l.startsWith("*.")) return [`F.${l.slice(2)}`, `B.${l.slice(2)}`];
  return [l];
}

/** The pad's layers; a pad that names none is on the front copper. */
const padLayers = (layers: readonly string[]): string[] => (layers.length > 0 ? layers : ["F.Cu"]).flatMap(padLayerSet);

/** `PAD::IsOnLayer`. */
export function fpPadOnLayer(layers: readonly string[], layer: string): boolean {
  return padLayers(layers).includes(layer);
}

/** `PAD::ViewGetLOD`: a pad is drawn while one of its layers is shown. */
export function fpPadShown(layers: readonly string[], layerVisible: Readonly<Record<string, boolean>>): boolean {
  return padLayers(layers).some((l) => layerIsVisible(layerVisible, l));
}

/** `PCB_TEXT::ViewGetLOD` for a text of the footprint: Footprint Text shows every text, a `${REFERENCE}` also needs References and a `${VALUE}` Values. */
export function fpTextShown(layerVisible: Readonly<Record<string, boolean>>, content: string): boolean {
  if (!objectOn(layerVisible, "footprint_text")) return false;
  if (content === "${REFERENCE}" && !objectOn(layerVisible, "footprint_references")) return false;
  if (content === "${VALUE}" && !objectOn(layerVisible, "footprint_values")) return false;
  return true;
}

/** `PCB_FIELD::ViewGetLOD`: the Reference needs References, the Value Values, and every field Footprint Text (the text rule it falls through to). */
export function fpFieldShown(layerVisible: Readonly<Record<string, boolean>>, name: string): boolean {
  if (!objectOn(layerVisible, "footprint_text")) return false;
  if (name === "Reference" && !objectOn(layerVisible, "footprint_references")) return false;
  if (name === "Value" && !objectOn(layerVisible, "footprint_values")) return false;
  return true;
}

/** `PCB_SHAPE::IsAnyFill` as the board's painter reads it: a filled rectangle, circle or polygon (a line, an arc and a curve have no inside). */
const shapeFilled = (s: { kind: string; filled: boolean }): boolean => s.filled && s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier";

/** `GetColor`'s per-type opacity for a shape of the footprint: a filled one takes Filled Shapes', a copper one Tracks', any other none. */
export function fpShapeOpacity(s: { kind: string; filled: boolean; layer: string }, layerOpacity: Readonly<Record<string, number>> | undefined): number {
  if (shapeFilled(s)) return fpObjectOpacity(layerOpacity, "shapes");
  return isCopper(s.layer) ? fpObjectOpacity(layerOpacity, "tracks") : 1;
}

/**
 * The Footprint Editor's selection filter (`FOOTPRINT_EDITOR_SETTINGS::m_SelectionFilter`, `editing.selection_filter`; `PCB_SELECTION_FILTER_OPTIONS`): the board panel's
 * rows (UI-LEFT-2 FF5). Kept with the editor's preferences, as KiCad keeps it in the editor's settings.
 */
export interface FpSelectionFilter {
  lockedItems: boolean;
  footprints: boolean;
  text: boolean;
  tracks: boolean;
  vias: boolean;
  pads: boolean;
  graphics: boolean;
  zones: boolean;
  keepouts: boolean;
  dimensions: boolean;
  points: boolean;
  otherItems: boolean;
}

/** The defaults of `editing.selection_filter` (footprint_editor_settings.cpp): every row on but Locked items. */
export const DEFAULT_FP_SELECTION_FILTER: FpSelectionFilter = { lockedItems: false, footprints: true, text: true, tracks: true, vias: true, pads: true, graphics: true, zones: true, keepouts: true, dimensions: true, points: true, otherItems: true };

/**
 * `PCB_SELECTION_TOOL::itemPassesFilter` for an item of the footprint (`kind` as `fpHitTest` names it): its row of the filter (a field, a text, a text box and a table
 * are Text, an image Graphics, a barcode Other items, a rule area Rule Areas), and, unless Locked items is on, not a locked item -- but a single pad clicked alone
 * ("allow a single pad to be selected -- there are a lot of operations that require this").
 */
export function fpPassesFilter(filter: FpSelectionFilter, kind: string, locked: boolean, ruleArea = false, multiSelect = false): boolean {
  if (!filter.lockedItems && locked && !(kind === "pad" && !multiSelect)) return false;
  switch (kind) {
    case "pad":
      return filter.pads;
    case "graphic":
    case "image":
      return filter.graphics;
    case "field":
    case "text":
    case "textbox":
    case "table":
      return filter.text;
    case "dimension":
      return filter.dimensions;
    case "zone":
      return ruleArea ? filter.keepouts : filter.zones;
    case "point":
      return filter.points;
    default:
      return filter.otherItems;
  }
}

/** What a click may pick (`fpHitTest`'s optional argument): the Appearance manager's switches, and the active layer when high contrast is on. */
export interface FpVisibility {
  /** The selection filter and the items' locks (UI-LEFT-2 FF5); absent, every kind passes. */
  filter?: FpSelectionFilter;
  layerVisible: Readonly<Record<string, boolean>>;
  layerOpacity?: Readonly<Record<string, number>>;
  /** High contrast (Dim or Hide): only an item on `activeLayer` is picked (`Selectable`: "We do not want to select items that are in the background"). */
  highContrast?: boolean;
  activeLayer?: string | null;
}

const zeroOpacity = (vis: FpVisibility, id: ObjectId): boolean => fpObjectOpacity(vis.layerOpacity, id) <= 0;
const offContrast = (vis: FpVisibility, onActive: boolean): boolean => !!vis.highContrast && !!vis.activeLayer && !onActive;

/** `Selectable` for an item on `layers` (one, or a zone's set): one of them shown, and with high contrast one of them the active layer. */
export function fpLayersSelectable(vis: FpVisibility, layers: readonly string[]): boolean {
  if (!layers.some((l) => layerIsVisible(vis.layerVisible, l))) return false;
  return !offContrast(vis, !!vis.activeLayer && layers.includes(vis.activeLayer));
}

/** `LSET::PhysicalLayersMask()` of the footprint editor's board: the copper and the board technical layers. */
const PHYSICAL_LAYERS: readonly string[] = [...FP_COPPER_LAYERS, "F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask"];

/**
 * `Selectable( PAD )` in the footprint editor (pcb_selection_tool.cpp at KiCad 8303b2ad, the same at 25ec2335): not at Pads opacity 0; a pad with a hole while any
 * physical layer is shown ("A pad's hole is visible on every layer the pad is visible on plus many layers the pad is not visible on"), another while one of its
 * layers is; with high contrast, on the active layer. The Pads row itself is not asked (UI-LEFT-2 FF5): it hides the view's pad layers only, and `Selectable` asks
 * no element visibility of a pad -- a pad hidden by the row is still picked, as KiCad's is.
 */
export function fpPadSelectable(vis: FpVisibility, layers: readonly string[], hasHole = false): boolean {
  if (zeroOpacity(vis, "pads")) return false;
  if (hasHole ? !PHYSICAL_LAYERS.some((l) => layerIsVisible(vis.layerVisible, l)) : !fpPadShown(layers, vis.layerVisible)) return false;
  return !offContrast(vis, !!vis.activeLayer && (hasHole ? isCopper(vis.activeLayer) || fpPadOnLayer(layers, vis.activeLayer) : fpPadOnLayer(layers, vis.activeLayer)));
}

/** `Selectable( PCB_FIELD )`. */
export function fpFieldSelectable(vis: FpVisibility, f: { name: string; layer: string }): boolean {
  return fpFieldShown(vis.layerVisible, f.name) && fpLayersSelectable(vis, [f.layer]);
}

/** `Selectable( PCB_TEXT )` of the footprint. */
export function fpTextSelectable(vis: FpVisibility, t: { content: string; layer: string }): boolean {
  return fpTextShown(vis.layerVisible, t.content) && fpLayersSelectable(vis, [t.layer]);
}

/** `Selectable( PCB_SHAPE )`: "m_FilledShapeOpacity == 0.0 && IsAnyFill()" is not picked. */
export function fpShapeSelectable(vis: FpVisibility, s: { kind: string; filled: boolean; layer: string }): boolean {
  if (shapeFilled(s) && zeroOpacity(vis, "shapes")) return false;
  return fpLayersSelectable(vis, [s.layer]);
}

/**
 * `Selectable( ZONE )`: not at Zones opacity 0, and a layer of the zone's set shown. `board()->IsElementVisible( LAYER_ZONES )` is the board's element visibility,
 * which the footprint editor's Zones row does not set (it hides the view's layer: `onObjectVisibilityChanged`), so a hidden zone is still picked (UI-LEFT-2 FF5).
 */
export function fpZoneSelectable(vis: FpVisibility, layers: readonly string[]): boolean {
  if (zeroOpacity(vis, "zones")) return false;
  return fpLayersSelectable(vis, layers);
}

/** `Selectable` for a text box, an image (the Images row, its opacity), a point (the Points row) or a barcode (kicad-port/pcbExtraItems.ts' kinds), and for a table or a dimension. */
export function fpItemSelectable(vis: FpVisibility, kind: string, layer: string): boolean {
  if (kind === "image" && (!objectOn(vis.layerVisible, "bitmaps") || zeroOpacity(vis, "bitmaps"))) return false;
  // a point: `board()->IsElementVisible( LAYER_POINTS )`, which the Points row does not set in this editor (as for a zone): its layer alone (UI-LEFT-2 FF5)
  return fpLayersSelectable(vis, [layer]);
}

// ------------------------------------------------------------------------------------------------------------------------------------ presets

/**
 * The Appearance manager's Presets in the Footprint Editor (`APPEARANCE_CONTROLS` with `m_isFpEditor`; the person's presets are `FOOTPRINT_EDITOR_SETTINGS::m_LayerPresets`,
 * UI-LEFT-2 FF5): the board's list and rules (kicad-port/layerPresets.ts) on the editor's layers and its Objects rows. What is on screen as a `PresetView`: the panel's layers,
 * the shown ones, the objects of `s_allowedInFpEditor` that are on (the grid the frame's switch), the active layer, and Flip Board View (`m_FlipBoardView`: a preset
 * records it and puts it back, `doApplyLayerPreset`'s `options.m_FlipBoardView = aPreset.flipBoard`, UI-LEFT-3 F3-9).
 */
export function fpPresetView(universe: readonly string[], s: { layerVisible: Readonly<Record<string, boolean>>; gridVisible: boolean; activeLayer: string; boardFlipped?: boolean }, activePreset = "", lastBuiltinObjects: readonly ObjectId[] | null = null): PresetView {
  return {
    universe,
    visibleLayers: new Set(universe.filter((l) => layerIsVisible(s.layerVisible, l))),
    objects: FP_EDITOR_OBJECTS.filter((id) => fpObjectChecked(s, id)),
    activeLayer: s.activeLayer,
    flipBoard: !!s.boardFlipped,
    activePreset,
    lastBuiltinObjects,
    boardLayers: new Set(universe),
  };
}

/** `syncLayerPresetSelection` in this editor: the first preset whose layers, objects (the Footprint Editor's rows of them) and flip are what is on screen, else "". */
export function fpMatchingPreset(presets: readonly LayerPreset[], v: PresetView): string {
  const universe = new Set(v.universe);
  const same = (a: Iterable<string>, b: Iterable<string>) => {
    const x = new Set(a);
    const y = [...b];
    return x.size === y.length && y.every((k) => x.has(k));
  };
  const fpObjects = (ids: readonly ObjectId[]) => ids.filter((id) => FP_EDITOR_OBJECTS.includes(id));
  return presets.find((p) => same(p.layers.filter((l) => universe.has(l)), [...v.visibleLayers]) && same(fpObjects(p.renderLayers), fpObjects(v.objects)) && !!p.flipBoard === v.flipBoard)?.name ?? "";
}

/** A preset applied (`selectPreset`'s view) as the editor's switches: the layers' and the objects' `layerVisible` entries, the grid switch, the active layer, the flip. */
export function fpApplyPresetView(universe: readonly string[], layerVisible: Readonly<Record<string, boolean>>, next: PresetView): { visible: Record<string, boolean>; grid: boolean; activeLayer: string | null; flipped: boolean } {
  let visible: Record<string, boolean> = { ...layerVisible, ...fpVisibleLayersPatch(universe, new Set(next.visibleLayers)) };
  for (const id of FP_EDITOR_OBJECTS) if (id !== "grid") visible = { ...visible, ...fpObjectPatch(visible, id, next.objects.includes(id)) };
  return { visible, grid: next.objects.includes("grid"), activeLayer: next.activeLayer, flipped: next.flipBoard };
}
