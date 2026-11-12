// Board Setup > Board Stackup > Physical Stackup, the parts of pcbnew/board_stackup_manager/panel_board_stackup.cpp that are pure: the rows the panel builds
// (`buildLayerStackPanel`, `lazyBuildRowUI`: one row per sublayer of a dielectric), Add Dielectric Layer and Remove Dielectric Layer (`onAddDielectricLayer`,
// `onRemoveDielectricLayer`, `onRemoveDielUI`, `BOARD_STACKUP_ITEM::AddDielectricPrms` / `RemoveDielectricPrms`), Adjust Dielectric Thickness
// (`onAdjustDielectricThickness`, `setDefaultLayerWidths`), the checks of `transferDataFromUIToStackup`, and what the board would lose with fewer copper layers
// (`PANEL_SETUP_LAYERS::TransferDataFromWindow`'s `getRemovedLayersWithItems` and the items it lists, `GetItemDescription( frame, true )`). The page is
// components/boardSetup/StackupPage.tsx; the backend stores the stackup (`set_stackup`, crates/ops/src/board_setup.rs) and deletes the items when told to.
//
// A layer's own `material`, `thickness_mm`, `thickness_locked`, `epsilon_r`, `loss_tangent` and `color` are its first sublayer; `sublayers` are the ones after it
// (crates/model/src/lib.rs `StackupLayer::sublayers`). Dependency-free (types only) so node --test runs it.
//
// Also here: what an item's parameters are when the board file does not say (`BOARD_STACKUP_ITEM( type )`, `DIELECTRIC_PRMS()`), the material lists
// (`DIELECTRIC_SUBSTRATE_LIST`, `DIALOG_DIELECTRIC_MATERIAL`, `onMaterialChange`), the colour lists (`GetStandardColors`, `createColorBox`, `getColorIconItem`),
// the values Apply writes (`transferDataFromUIToStackup`) and Export to Clipboard's text (`BuildStackupReport`).
import type { BoardState, Nm, Side, Stackup, StackupLayer, StackupSettings, StackupSublayer } from "../api/types";
import { messageTextFromValue } from "./messageText";
import { stringFromValue } from "./fpFields";
import { uiDouble2Str } from "./numericEvaluator";

export const NOT_SPECIFIED = "Not specified";

/** `BOARD_STACKUP_ITEM_TYPE`, as `PCB_IO_KICAD_SEXPR_PARSER::parseBoardStackup` gives a layer from its name: silkscreen, mask and paste by their board layer, a
 * copper layer, and a dielectric for a name that is no board layer ("dielectric 1"). */
export type StackupItemType = "copper" | "dielectric" | "solderpaste" | "soldermask" | "silkscreen";

export function stackupItemType(name: string): StackupItemType {
  switch (name) {
    case "F.SilkS":
    case "B.SilkS":
    case "F.Silkscreen":
    case "B.Silkscreen":
      return "silkscreen";
    case "F.Mask":
    case "B.Mask":
      return "soldermask";
    case "F.Paste":
    case "B.Paste":
      return "solderpaste";
    default:
      return /^(F|B|In([1-9]|[12]\d|30))\.Cu$/.test(name) ? "copper" : "dielectric";
  }
}

/** `IsThicknessEditable`: copper, a dielectric and a solder mask. */
export function itemThicknessEditable(name: string): boolean {
  const t = stackupItemType(name);
  return t === "copper" || t === "dielectric" || t === "soldermask";
}

/** `IsMaterialEditable` (and `IsColorEditable`): a dielectric, a solder mask and a silkscreen. */
export function itemMaterialEditable(name: string): boolean {
  const t = stackupItemType(name);
  return t === "dielectric" || t === "soldermask" || t === "silkscreen";
}

/** `HasEpsilonRValue` (and `HasLossTangentValue`): a dielectric and a solder mask. */
export function itemHasEpsilonR(name: string): boolean {
  const t = stackupItemType(name);
  return t === "dielectric" || t === "soldermask";
}

/** `IsPrmSpecified`: not empty and not "Not specified". */
export function prmSpecified(v: string | null | undefined): boolean {
  return v !== null && v !== undefined && v !== "" && v.toLowerCase() !== NOT_SPECIFIED.toLowerCase();
}

/** `GetSublayersCount()`. */
export function sublayerCount(l: StackupLayer): number {
  return 1 + (l.sublayers?.length ?? 0);
}

/** Sublayer `i` of the layer (`m_DielectricPrmsList[i]`): 0 is the layer's own fields. */
export function sublayerOf(l: StackupLayer, i: number): StackupSublayer {
  if (i === 0) {
    const s: StackupSublayer = { material: l.material, thickness_mm: l.thickness_mm };
    if (l.thickness_locked) s.thickness_locked = true;
    if (l.epsilon_r !== undefined) s.epsilon_r = l.epsilon_r;
    if (l.loss_tangent !== undefined) s.loss_tangent = l.loss_tangent;
    if (l.color !== undefined) s.color = l.color;
    return s;
  }
  return l.sublayers?.[i - 1] ?? {};
}

/** The layer with sublayer `i`'s fields changed by `patch` (a key set to `undefined` is taken away). */
export function withSublayer(l: StackupLayer, i: number, patch: Partial<StackupSublayer>): StackupLayer {
  const drop = <T extends object>(o: T): T => Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined)) as T;
  if (i === 0) {
    const next = drop({ ...l, ...patch } as StackupLayer);
    if (next.material === undefined) next.material = null;
    if (next.thickness_mm === undefined) next.thickness_mm = null;
    if (next.thickness_locked === false) delete next.thickness_locked;
    return next;
  }
  const subs = [...(l.sublayers ?? [])];
  const merged = drop({ ...subs[i - 1], ...patch });
  if (merged.thickness_locked === false) delete merged.thickness_locked;
  subs[i - 1] = merged;
  return { ...l, sublayers: subs };
}

/** Every sublayer's thickness, nm (a sublayer that has none is 0). */
export function sublayerThicknessesNm(l: StackupLayer): Nm[] {
  return Array.from({ length: sublayerCount(l) }, (_, i) => Math.round((sublayerOf(l, i).thickness_mm ?? 0) * 1e6));
}

/** `BuildBoardThicknessFromStackup`: every layer's thickness with its sublayers', nm. */
export function stackupTotalThicknessNm(s: Stackup): Nm {
  return Math.round(s.layers.reduce((sum, l) => sum + (l.thickness_mm ?? 0) + (l.sublayers ?? []).reduce((a, x) => a + (x.thickness_mm ?? 0), 0), 0) * 1e6);
}

/** `BOARD_STACKUP_ITEM::FormatDielectricLayerName`: "Dielectric %d". */
export function formatDielectricLayerName(id: number): string {
  return `Dielectric ${id}`;
}

/** One row of the panel's grid: sublayer `sub` of layer `layer` (`BOARD_STACKUP_ROW_UI_ITEM( item, sub_idx, row )`). */
export interface StackupRow {
  layer: number;
  sub: number;
  /** `GetSublayersCount()` of the layer. */
  count: number;
  type: StackupItemType;
  /** `GetDielectricLayerId()`: a dielectric's place among the dielectrics, from 1 (`parseBoardStackup`'s `dielectric_idx++`); 0 for another layer. */
  dielectricId: number;
  /** The Layer column: "Dielectric N" (and "  (k/n)" for a layer of sublayers), else the layer's name (`m_board->GetLayerName`). */
  label: string;
  /** The Type column: `"choice"` for the Core / PrePreg choice of a dielectric's first sublayer, `""` for its other sublayers ("the layer type choice is not
   * for each sublayer, only for the first"), "Copper" for copper and the type's name for the rest. */
  typeLabel: string;
}

/** `buildLayerStackPanel` + `lazyBuildRowUI`: a row for each sublayer of each layer, top to bottom. `layerName` is `BOARD::GetLayerName` (the name the user gave it). */
export function stackupRows(s: Stackup, layerName: (file: string) => string = (f) => f): StackupRow[] {
  const rows: StackupRow[] = [];
  let dielectrics = 0;
  s.layers.forEach((l, layer) => {
    const type = stackupItemType(l.name);
    const dielectricId = type === "dielectric" ? ++dielectrics : 0;
    const count = sublayerCount(l);
    for (let sub = 0; sub < count; sub++) {
      let label: string;
      let typeLabel: string;
      if (type === "dielectric") {
        label = formatDielectricLayerName(dielectricId);
        if (count > 1) label += `  (${sub + 1}/${count})`;
        typeLabel = sub === 0 ? "choice" : "";
      } else {
        label = layerName(l.name);
        typeLabel = l.kind === "copper" ? "Copper" : (l.kind ?? "");
      }
      rows.push({ layer, sub, count, type, dielectricId, label, typeLabel });
    }
  });
  return rows;
}

/** `m_core_prepreg_choice`: the Type choice of a dielectric. */
export const CORE_PREPREG = ["Core", "PrePreg"] as const;

/** `synchronizeWithBoard`'s `choice->SetSelection( item->GetTypeName() == KEY_CORE ? 0 : 1 )`. */
export function corePrepregOf(kind: string | undefined): "Core" | "PrePreg" {
  return kind === "core" ? "Core" : "PrePreg";
}

/** `transferDataFromUIToStackup`: index 0 is `KEY_CORE`, else `KEY_PREPREG`. */
export function kindOfChoice(choice: string): "core" | "prepreg" {
  return choice === "Core" ? "core" : "prepreg";
}

/** An item of the Add or Remove Dielectric Layer list: its text and the sublayer it stands for. */
export interface DielectricChoice {
  label: string;
  layer: number;
  sub: number;
}

/** `onAddDielectricLayer`'s list: every row of a dielectric, "Layer '%s' (sublayer %d/%d)" for a layer of sublayers, else the layer's name. */
export function addDielectricChoices(s: Stackup): DielectricChoice[] {
  return stackupRows(s)
    .filter((r) => r.type === "dielectric")
    .map((r) => ({ layer: r.layer, sub: r.sub, label: r.count > 1 ? `Layer '${formatDielectricLayerName(r.dielectricId)}' (sublayer ${r.sub + 1}/${r.count})` : formatDielectricLayerName(r.dielectricId) }));
}

/** `onRemoveDielectricLayer`'s list: each sublayer of the dielectrics that have more than one, "Layer '%s' sublayer %d/%d". */
export function removeDielectricChoices(s: Stackup): DielectricChoice[] {
  return stackupRows(s)
    .filter((r) => r.type === "dielectric" && r.count > 1)
    .map((r) => ({ layer: r.layer, sub: r.sub, label: `Layer '${formatDielectricLayerName(r.dielectricId)}' sublayer ${r.sub + 1}/${r.count}` }));
}

/** `onRemoveDielUI`: Remove Dielectric Layer is enabled only while a dielectric has sublayers. */
export function canRemoveDielectric(s: Stackup): boolean {
  return s.layers.some((l) => stackupItemType(l.name) === "dielectric" && sublayerCount(l) > 1);
}

/** `DIELECTRIC_PRMS()`, what `AddDielectricPrms` inserts: thickness 0, unlocked, epsilon R 1, loss tangent 0, no material or colour. */
export function newSublayer(): StackupSublayer {
  return { thickness_mm: 0, epsilon_r: 1, loss_tangent: 0 };
}

/** Add Dielectric Layer on the chosen row: "Insert a new item after the selected item" (`AddDielectricPrms( new_sublayer + 1 )`). */
export function addDielectricSublayer(s: Stackup, layer: number, sub: number): Stackup {
  return {
    ...s,
    layers: s.layers.map((l, i) => {
      if (i !== layer) return l;
      const subs = [...(l.sublayers ?? [])];
      subs.splice(sub, 0, newSublayer());
      return { ...l, sublayers: subs };
    }),
  };
}

/** Remove Dielectric Layer on the chosen sublayer (`RemoveDielectricPrms`; nothing when the layer has one only). Removing the first makes the next one the layer's own. */
export function removeDielectricSublayer(s: Stackup, layer: number, sub: number): Stackup {
  return {
    ...s,
    layers: s.layers.map((l, i) => {
      const count = sublayerCount(l);
      if (i !== layer || count < 2 || sub < 0 || sub >= count) return l;
      const all = Array.from({ length: count }, (_, k) => sublayerOf(l, k));
      all.splice(sub, 1);
      const [first, ...rest] = all;
      let next: StackupLayer = { name: l.name, material: null, thickness_mm: null, ...(l.kind !== undefined ? { kind: l.kind } : {}) };
      next = withSublayer(next, 0, { material: first!.material ?? null, thickness_mm: first!.thickness_mm ?? null, thickness_locked: first!.thickness_locked, epsilon_r: first!.epsilon_r, loss_tangent: first!.loss_tangent, color: first!.color });
      if (rest.length > 0) next.sublayers = rest;
      return next;
    }),
  };
}

/**
 * `transferDataFromUIToStackup`'s checks, in its words and its order, row by row: the epsilon R and the loss tangent of a layer that has them, then the thickness of
 * a layer that has one. `null` when all is well, else the text of the "Errors" message box. A blank epsilon R or loss tangent is this model's "not written"
 * (`None`), which is accepted; KiCad's field always holds a number ("positive or null if not used").
 */
export function stackupEntryErrors(s: Stackup): string | null {
  let msg = "";
  const bad = (v: number | null | undefined) => v !== undefined && v !== null && (!Number.isFinite(v) || v < 0);
  for (const row of stackupRows(s)) {
    const l = s.layers[row.layer]!;
    const sub = sublayerOf(l, row.sub);
    if (itemHasEpsilonR(l.name)) {
      if (bad(sub.epsilon_r)) msg += "Incorrect value for Epsilon R (Epsilon R must be positive or null if not used)";
      if (bad(sub.loss_tangent)) {
        if (msg !== "") msg += "\n";
        msg += "Incorrect value for Loss tg (Loss tg must be positive or null if not used)";
      }
    }
    if (itemThicknessEditable(l.name)) {
      const t = sub.thickness_mm;
      if (t !== null && t !== undefined && (!Number.isFinite(t) || t < 0)) {
        if (msg !== "") msg += "\n";
        msg += "A layer thickness is < 0. Fix it";
      }
    }
  }
  return msg === "" ? null : msg;
}

// ------------------------------------------------------------------------------------------------------------------ Adjust Dielectric Thickness

type Units = "mm" | "mil" | "in";

const UNIT_TEXT: Record<Units, string> = { mm: "mm", mil: "mils", in: "in" };

/** Whether the row's thickness is locked: a dielectric row with its lock box ticked. */
function rowLocked(s: Stackup, r: StackupRow): boolean {
  return r.type === "dielectric" && sublayerOf(s.layers[r.layer]!, r.sub).thickness_locked === true;
}

/** `onAdjustDielectricThickness` before its dialog: the least thickness the board can have (the copper, the masks and the locked dielectrics), the dialog's prompt
 * and title, and whether any dielectric is unlocked. */
export function adjustThicknessPrompt(s: Stackup, units: Units): { minNm: Nm; prompt: string; title: string; unlocked: number } {
  let min = 0;
  let unlocked = 0;
  for (const r of stackupRows(s)) {
    const l = s.layers[r.layer]!;
    if (!itemThicknessEditable(l.name)) continue;
    // "We are looking for locked thickness items only": a dielectric's row has the lock box
    if (r.type === "dielectric" && !rowLocked(s, r)) {
      unlocked++;
      continue;
    }
    min += Math.round((sublayerOf(l, r.sub).thickness_mm ?? 0) * 1e6);
  }
  const prompt = min === 0 ? `Enter board thickness in ${UNIT_TEXT[units]}:` : `Enter expected board thickness (min value ${stringFromValue(min, units)}):`;
  return { minNm: min, prompt, title: "Adjust Unlocked Dielectric Layers", unlocked };
}

/**
 * `PANEL_SETUP_BOARD_STACKUP::setDefaultLayerWidths( targetThickness )`: a symmetrical stackup of the even copper count, prepreg of 0.1 mm on the outside (a core
 * for a 2-layer board), cores taking what is left after the fixed items (copper, masks, sublayers after the first, locked dielectrics) and the prepregs; when a core
 * would be thinner than the prepreg, every dielectric gets an equal share. Each unlocked dielectric's first sublayer takes its thickness and its type (Core or
 * PrePreg, alternating); a locked one keeps its own, and the alternation still steps over it. Integer nanometres, as KiCad's `int` arithmetic is.
 */
export function setDefaultLayerWidths(s: Stackup, copperLayers: number, targetNm: Nm): Stackup {
  const prePregDefault = 100_000;
  const dielectricCount = copperLayers - 1;
  let coreCount = Math.trunc(copperLayers / 2) - 1;
  let currentIsCore = false;
  if (copperLayers === 2) {
    coreCount = 1;
    currentIsCore = true;
  }
  const prePregCount = dielectricCount - coreCount;
  const rows = stackupRows(s);
  let fixed = 0;
  for (const r of rows) {
    const l = s.layers[r.layer]!;
    if (!itemThicknessEditable(l.name)) continue;
    if ((r.type === "dielectric" && r.sub > 0) || r.type === "soldermask" || r.type === "copper" || rowLocked(s, r)) fixed += Math.round((sublayerOf(l, r.sub).thickness_mm ?? 0) * 1e6);
  }
  let remaining = targetNm - fixed - prePregDefault * prePregCount;
  let prePreg = prePregDefault;
  let core = Math.trunc(remaining / coreCount);
  if (core < prePreg) {
    remaining = targetNm - fixed;
    prePreg = core = Math.max(0, Math.trunc(remaining / dielectricCount));
  }
  const layers = [...s.layers];
  for (const r of rows) {
    if (r.type !== "dielectric" || r.sub !== 0) continue;
    if (rowLocked(s, r)) {
      currentIsCore = !currentIsCore;
      continue;
    }
    const t = currentIsCore ? core : prePreg;
    layers[r.layer] = { ...withSublayer(layers[r.layer]!, 0, { thickness_mm: t / 1e6 }), kind: currentIsCore ? "core" : "prepreg" };
    currentIsCore = !currentIsCore;
  }
  return { ...s, layers };
}

/**
 * `PANEL_SETUP_BOARD_STACKUP::onCopperLayersSelCount`: the stackup for the new copper count (`withCopper`, the layers that remain keeping what they say), then
 * `setDefaultLayerWidths( oldBoardWidth )` -- the unlocked dielectrics share the thickness the board had before, so the board keeps its thickness.
 */
export function withCopperLayerCount<T extends { copper_layers: number; stackup: Stackup }>(settings: T, copperLayers: number, withCopper: (s: T, n: number) => T): T {
  const old = stackupTotalThicknessNm(settings.stackup);
  const next = withCopper(settings, copperLayers);
  return { ...next, stackup: setDefaultLayerWidths(next.stackup, copperLayers, old) };
}

/** `onAdjustDielectricThickness` after its dialog: the value entered (nm), refused below the least ("Value too small (min value %s)."), else the unlocked
 * dielectrics made to fit (`setDefaultLayerWidths`), or "All dielectric  thickness layers are locked" when none is unlocked. */
export function adjustDielectricThickness(s: Stackup, copperLayers: number, valueNm: Nm, units: Units): { stackup: Stackup | null; message: string | null } {
  const { minNm, unlocked } = adjustThicknessPrompt(s, units);
  if (valueNm < minNm) return { stackup: null, message: `Value too small (min value ${stringFromValue(minNm, units)}).` };
  if (unlocked === 0) return { stackup: null, message: "All dielectric  thickness layers are locked" };
  return { stackup: setDefaultLayerWidths(s, copperLayers, valueNm), message: null };
}

// ------------------------------------------------------------------------------------------------------------------ removed copper layers

/** `BOARD_CONNECTED_ITEM::GetNetnameMsg`: "[net]", or "[<no net>]". */
export function netnameMsg(net: string | null | undefined): string {
  return net ? `[${net}]` : "[<no net>]";
}

/** `EDA_SHAPE::getFriendlyName`. */
const SHAPE_NAMES: Record<string, string> = { segment: "Segment", arc: "Arc", rect: "Rectangle", circle: "Circle", polygon: "Polygon", bezier: "Curve" };

/** Copper layers of a board of `n`, outer first. */
function copperNames(n: number): string[] {
  const names = ["F.Cu"];
  for (let i = 1; i < n - 1; i++) names.push(`In${i}.Cu`);
  names.push("B.Cu");
  return names;
}

/** The length of an arc through `a`, `m`, `b` (`PCB_ARC::GetLength`: radius times the angle swept), nm; the chord when the three are on a line. */
function arcLength(a: [number, number], m: [number, number], b: [number, number]): number {
  const [ax, ay] = a;
  const [bx, by] = m;
  const [cx, cy] = b;
  const d = 2 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
  if (Math.abs(d) < 1e-9) return Math.hypot(cx - ax, cy - ay);
  const ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
  const uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
  const r = Math.hypot(ax - ux, ay - uy);
  const ang = (p: [number, number]) => Math.atan2(p[1] - uy, p[0] - ux);
  const norm = (t: number) => ((t % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
  const sweepAM = norm(ang(m) - ang(a));
  const sweepAB = norm(ang(b) - ang(a));
  const sweep = sweepAM <= sweepAB ? sweepAB : 2 * Math.PI - sweepAB;
  return r * sweep;
}

/** `ZONE::GetItemDescription`'s layer part: "on %s", "on %s and %s", "on %s, %s and %s", "on %s, %s and %zu more". */
function zoneLayerDesc(layers: string[], name: (f: string) => string): string {
  const n = layers.map(name);
  if (n.length === 1) return `on ${n[0]}`;
  if (n.length === 2) return `on ${n[0]} and ${n[1]}`;
  if (n.length === 3) return `on ${n[0]}, ${n[1]} and ${n[2]}`;
  return `on ${n[0]}, ${n[1]} and ${n.length - 2} more`;
}

/** The parts of the board state the question needs. */
export type RemovedLayerBoard = Pick<BoardState, "routing" | "drawings"> & { parts?: BoardState["parts"] };

/** One entry of the question's list: its text (`GetItemDescription( frame, true )`, or "Pad %s of %s on %s") and the id of the item it is, which a click on it
 * selects (`SetSelectionCallback`). */
export interface RemovedLayerItem {
  text: string;
  id: string;
}

/** The id a pad is selected under (kicad-port/pcbItems.ts `padIds`, repeated here to keep this file free of the canvas code): `REF.NUM`, `#k` for the k-th of a number. */
function padIdsOf(part: { ref: string; pads?: readonly { num: string; id?: string }[] }): string[] {
  const seen = new Map<string, number>();
  return (part.pads ?? []).map((pad) => {
    const n = (seen.get(pad.num) ?? 0) + 1;
    seen.set(pad.num, n);
    return pad.id ?? (n === 1 ? `${part.ref}.${pad.num}` : `${part.ref}.${pad.num}#${n}`);
  });
}

/**
 * Fewer copper layers: `PANEL_SETUP_LAYERS::TransferDataFromWindow`'s list of what is on the layers that go, each as `GetItemDescription( frame, true )` says it.
 * The layers are those of `getRemovedLayersWithItems` -- the ones that go and have something on them (`BOARD::HasItemsOnLayer`: a through via does not count, nor
 * a blind or buried via that only passes through) -- in the board's order; for each, the board-level items on it (`PCB_LAYER_COLLECTOR` over `BoardLevelItems`,
 * `IsOnLayer`), so a via is listed for every layer its drill spans, a through via too, and a zone on several layers for each that goes. The order of a layer's
 * items is `BOARD::Visit`'s: the drawings (shapes, texts, text boxes, tables, dimensions, barcodes), the tracks and vias, the points, the zones, the generators.
 * Then the pads of the placed footprints that have a shape of their own on the layer (`pad->HasExplicitDefinitionForLayer( layer )`, [`padHasExplicitLayer`]):
 * "Pad %s of %s on %s". A track of several segments is listed once per segment and selected whole (the studio's track is the polyline).
 */
export function removedLayerItems(board: RemovedLayerBoard | null | undefined, fromLayers: number, toLayers: number, units: Units = "mm", layerName: (file: string) => string = (f) => f): RemovedLayerItem[] {
  if (!board || toLayers >= fromLayers) return [];
  const stack = copperNames(fromLayers);
  const keep = new Set(copperNames(toLayers));
  const at = (l: string) => stack.indexOf(l);
  const rt = board.routing;
  const dr = board.drawings;
  // `PCB_VIA::IsOnLayer`: a copper layer between the drill's start and end (a through via's are F.Cu and B.Cu, `SanitizeLayers`)
  const viaSpan = (v: { from: string; to: string; type?: string | null }): [string, string] => {
    if (!v.type) return ["F.Cu", "B.Cu"];
    return at(v.from) <= at(v.to) ? [v.from, v.to] : [v.to, v.from];
  };
  const viaOn = (v: { from: string; to: string; type?: string | null }, layer: string) => {
    const [top, bottom] = viaSpan(v);
    return at(top) <= at(layer) && at(layer) <= at(bottom);
  };
  const zoneSet = (z: { layer: string; layers?: string[] }) => (z.layers && z.layers.length > 0 ? z.layers : [z.layer]);
  // `BOARD::HasItemsOnLayer`
  const hasItems = (layer: string) =>
    (dr?.shapes ?? []).some((x) => x.layer === layer) ||
    (dr?.texts ?? []).some((x) => x.layer === layer) ||
    (dr?.text_boxes ?? []).some((x) => x.layer === layer) ||
    (dr?.tables ?? []).some((x) => x.layer === layer) ||
    (dr?.dimensions ?? []).some((x) => x.layer === layer) ||
    (dr?.barcodes ?? []).some((x) => x.layer === layer) ||
    (dr?.points ?? []).some((x) => x.layer === layer) ||
    (dr?.tuning_patterns ?? []).some((x) => x.layer === layer) ||
    (rt?.tracks ?? []).some((x) => x.layer === layer) ||
    (rt?.zones ?? []).some((z) => zoneSet(z).includes(layer)) ||
    (rt?.vias ?? []).some((v) => !!v.type && (v.from === layer || v.to === layer));
  const out: RemovedLayerItem[] = [];
  const add = (text: string, id: string) => out.push({ text, id });
  for (const layer of stack.filter((l) => !keep.has(l) && hasItems(l))) {
    const on = layerName(layer);
    for (const s of dr?.shapes ?? []) if (s.layer === layer) add(`${SHAPE_NAMES[s.kind] ?? "Unrecognized"} ${netnameMsg(s.net)} on ${on}`, s.id);
    for (const t of dr?.texts ?? []) if (t.layer === layer) add(`PCB text '${t.shown ?? t.content}' on ${on}`, t.id);
    for (const t of dr?.text_boxes ?? []) if (t.layer === layer) add(`PCB text box '${t.shown ?? t.text}' on ${on}`, t.id);
    for (const t of dr?.tables ?? []) if (t.layer === layer) add(`${t.cols} column table`, t.id);
    for (const d of dr?.dimensions ?? []) if (d.layer === layer) add(`Dimension '${d.text}' on ${on}`, d.id);
    for (const b of dr?.barcodes ?? []) if (b.layer === layer) add(`Barcode on ${on}`, b.id);
    for (const t of rt?.tracks ?? []) {
      if (t.layer !== layer) continue;
      if (t.arc_mid && t.pts.length >= 2) {
        add(`Track (arc) ${netnameMsg(t.net)} on ${on}, length ${messageTextFromValue(arcLength(t.pts[0]!, t.arc_mid, t.pts[t.pts.length - 1]!), units)}`, t.id);
        continue;
      }
      for (let i = 0; i + 1 < t.pts.length; i++) {
        const [a, b] = [t.pts[i]!, t.pts[i + 1]!];
        add(`Track ${netnameMsg(t.net)} on ${on}, length ${messageTextFromValue(Math.hypot(b[0] - a[0], b[1] - a[1]), units)}`, t.id);
      }
    }
    for (const v of rt?.vias ?? []) {
      if (!viaOn(v, layer)) continue;
      const word = v.type === "blind" ? "Blind via" : v.type === "buried" ? "Buried via" : v.type === "micro" ? "Micro via" : "Via";
      // `PCB_VIA::LayerMaskDescribe`: the top and the bottom layer of `LayerPair`
      const [top, bottom] = viaSpan(v);
      add(`${word} ${netnameMsg(v.net)} on ${layerName(top)} - ${layerName(bottom)}`, v.id);
    }
    for (const p of dr?.points ?? []) if (p.layer === layer) add(`Point on ${on}`, p.id);
    for (const z of rt?.zones ?? []) {
      const set = zoneSet(z);
      if (!set.includes(layer)) continue;
      const desc = zoneLayerDesc(set, layerName);
      const name = z.name ?? "";
      if (z.is_rule_area) add(name === "" ? `Rule Area ${desc}` : `Rule area '${name}' ${desc}`, z.id);
      else if (z.teardrop) add(`Teardrop ${netnameMsg(z.net)} ${desc}`, z.id);
      else add(name === "" ? `Zone ${netnameMsg(z.net)} ${desc}, priority ${z.priority}` : `Zone '${name}' ${netnameMsg(z.net)} ${desc}, priority ${z.priority}`, z.id);
    }
    // `PCB_GENERATOR::GetItemDescription`: "Generator"
    for (const p of dr?.tuning_patterns ?? []) if (p.layer === layer) add("Generator", p.id);
    // `for( FOOTPRINT* footprint : m_pcb->Footprints() ) for( PAD* pad : footprint->Pads() )`
    for (const part of board.parts ?? []) {
      if (!part.placed) continue;
      const ids = padIdsOf(part);
      (part.pads ?? []).forEach((pad, i) => {
        if (padHasExplicitLayer(pad, part.side, layer, fromLayers)) add(`Pad ${pad.num} of ${part.ref} on ${on}`, ids[i]!);
      });
    }
  }
  return out;
}

/** The texts of [`removedLayerItems`]. */
export function itemsOnRemovedLayers(board: RemovedLayerBoard | null | undefined, fromLayers: number, toLayers: number, units: Units = "mm", layerName: (file: string) => string = (f) => f): string[] {
  return removedLayerItems(board, fromLayers, toLayers, units, layerName).map((i) => i.text);
}

/** The text of KiCad's question (`DIALOG_ITEMS_LIST( parent, _( "Warning" ), .., _( "Show Details" ) )`). */
export const REMOVED_LAYERS_TITLE = "Warning";
export const REMOVED_LAYERS_MESSAGE = "Items have been found on removed layers. This operation will delete all items from removed layers and cannot be undone.";
export const REMOVED_LAYERS_DETAILS = "Show Details";

// ------------------------------------------------------------------------------------------------------------------ an item's parameters as KiCad has them

/** `DEFAULT_EPSILON_R_SOLDERMASK`, `DEFAULT_EPSILON_R_SILKSCREEN` (stackup_predefined_prms.h). */
export const DEFAULT_EPSILON_R_SOLDERMASK = 3.3;
export const DEFAULT_EPSILON_R_SILKSCREEN = 1.0;

/** One sublayer's parameters as KiCad holds them (`DIELECTRIC_PRMS`): never blank. */
export interface KicadPrms {
  material: string;
  thicknessNm: Nm;
  thicknessLocked: boolean;
  epsilonR: number;
  lossTangent: number;
  color: string;
}

/**
 * Sublayer `sub` of the layer as KiCad has it once the board is read: what the file says, else what `BOARD_STACKUP_ITEM( type )` starts the first sublayer with
 * (a dielectric FR4, epsilon R 4.5, loss tangent 0.02, colour "Not specified"; a mask "Not specified", 0.01 mm, epsilon R 3.3; a silkscreen "Not specified", epsilon R
 * 1.0; copper 0.035 mm) or, for a later sublayer, `DIELECTRIC_PRMS()` (no material or colour, thickness 0, epsilon R 1, loss tangent 0). This model leaves out what the
 * file leaves out, so a blank value here is the type's default, which is what KiCad shows (`lazyBuildRowUI`: `UIDouble2Str( item->GetEpsilonR( sublayerIdx ) )`).
 */
export function kicadPrms(l: StackupLayer, sub: number): KicadPrms {
  const type = stackupItemType(l.name);
  let d: KicadPrms = { material: "", thicknessNm: 0, thicknessLocked: false, epsilonR: 1.0, lossTangent: 0.0, color: "" };
  if (sub === 0) {
    if (type === "copper") d = { ...d, thicknessNm: 35_000 };
    else if (type === "dielectric") d = { ...d, color: NOT_SPECIFIED, material: "FR4", lossTangent: 0.02, epsilonR: 4.5 };
    else if (type === "soldermask") d = { ...d, color: NOT_SPECIFIED, material: NOT_SPECIFIED, thicknessNm: 10_000, epsilonR: DEFAULT_EPSILON_R_SOLDERMASK };
    else if (type === "silkscreen") d = { ...d, color: NOT_SPECIFIED, material: NOT_SPECIFIED, epsilonR: DEFAULT_EPSILON_R_SILKSCREEN };
  }
  const s = sublayerOf(l, sub);
  return {
    material: s.material ?? d.material,
    thicknessNm: s.thickness_mm === null || s.thickness_mm === undefined ? d.thicknessNm : Math.round(s.thickness_mm * 1e6),
    thicknessLocked: s.thickness_locked === true,
    epsilonR: s.epsilon_r ?? d.epsilonR,
    lossTangent: s.loss_tangent ?? d.lossTangent,
    color: s.color ?? d.color,
  };
}

/** `BOARD_STACKUP_ITEM::FormatEpsilonR` / `FormatLossTangent`: `UIDouble2Str`, no scientific notation for the usual values. */
export const formatPrm = uiDouble2Str;

/** `wxString::ToDouble`: the whole text is a number (`strtod` skipping leading blanks), else null. */
export function toDouble(text: string): number | null {
  return /^\s*[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(text) ? Number(text.trim()) : null;
}

// ------------------------------------------------------------------------------------------------------------------ colours

/** A layer kind's colour list (`GetStandardColors`): a silkscreen's and a mask's, a dielectric's; the others have none (`IsColorEditable`). */
export type ColorListType = "silkscreen" | "soldermask" | "dielectric";

/** `FAB_LAYER_COLOR`: a name and its colour (red, green, blue, 0..255). */
export interface FabLayerColor {
  name: string;
  rgb: readonly [number, number, number];
}

/** "User defined", the last entry of each colour list (`GetColorUserDefinedListIdx`). */
export const USER_DEFINED = "User defined";

/** `gbrjobColors` (stackup_predefined_prms.cpp): a solder mask's and a silkscreen's. */
const GBRJOB_COLORS: readonly FabLayerColor[] = [
  { name: NOT_SPECIFIED, rgb: [80, 80, 80] },
  { name: "Green", rgb: [60, 150, 80] },
  { name: "Red", rgb: [128, 0, 0] },
  { name: "Blue", rgb: [0, 0, 128] },
  { name: "Purple", rgb: [80, 0, 80] },
  { name: "Black", rgb: [20, 20, 20] },
  { name: "White", rgb: [200, 200, 200] },
  { name: "Yellow", rgb: [128, 128, 0] },
  { name: USER_DEFINED, rgb: [128, 128, 128] },
];

/** `dielectricColors` (their alpha is not used: `FAB_LAYER_COLOR::GetColor` gives a dielectric alpha 1). */
const DIELECTRIC_COLORS: readonly FabLayerColor[] = [
  { name: NOT_SPECIFIED, rgb: [80, 80, 80] },
  { name: "FR4 natural", rgb: [109, 116, 75] },
  { name: "PTFE natural", rgb: [252, 252, 250] },
  { name: "Polyimide", rgb: [205, 130, 0] },
  { name: "Phenolic natural", rgb: [92, 17, 6] },
  { name: "Aluminum", rgb: [213, 213, 213] },
  { name: USER_DEFINED, rgb: [128, 128, 128] },
];

/** The colour list of a layer, or null for one whose colour is not editable (`IsColorEditable`: a dielectric, a solder mask, a silkscreen). */
export function colorListType(name: string): ColorListType | null {
  const t = stackupItemType(name);
  return t === "silkscreen" || t === "soldermask" || t === "dielectric" ? t : null;
}

/** `GetStandardColors( aType )`. */
export function standardColors(type: ColorListType): readonly FabLayerColor[] {
  return type === "dielectric" ? DIELECTRIC_COLORS : GBRJOB_COLORS;
}

/** `DEFAULT_SOLDERMASK_OPACITY`. */
const SOLDERMASK_OPACITY = 0.83;

/** `FAB_LAYER_COLOR::GetColor( aItemType )` as CSS: a solder mask's colour at 0.83 opacity, any other opaque. */
export function standardColorCss(type: ColorListType, idx: number): string {
  const [r, g, b] = standardColors(type)[idx]!.rgb;
  return `rgba(${r}, ${g}, ${b}, ${type === "soldermask" ? SOLDERMASK_OPACITY : 1})`;
}

const hex2 = (n: number) => n.toString(16).toUpperCase().padStart(2, "0");

/** `GetDefaultUserColor( aType ).ToHexString()`: the "User defined" entry's grey, `#808080D4` for a mask (its opacity), `#808080FF` otherwise. */
export function defaultUserColor(type: ColorListType): string {
  const [r, g, b] = standardColors(type).at(-1)!.rgb;
  return `#${hex2(r)}${hex2(g)}${hex2(b)}${hex2(Math.round((type === "soldermask" ? SOLDERMASK_OPACITY : 1) * 255))}`;
}

/** `lazyBuildRowUI`'s selection of the colour box: "User defined" for a `#...` colour, the entry whose name is the colour's, else 0 ("Not specified"). */
export function colorIndex(type: ColorListType, color: string): number {
  const list = standardColors(type);
  if (color.startsWith("#")) return list.length - 1;
  const i = list.findIndex((c) => c.name === color);
  return i < 0 ? 0 : i;
}

/** A `#RRGGBB[AA]` colour as CSS (`COLOR4D( aColorStr )`), or null. */
export function hexColorCss(color: string): string | null {
  const m = /^#([0-9a-fA-F]{6})([0-9a-fA-F]{2})?$/.exec(color.trim());
  if (!m) return null;
  const v = Number.parseInt(m[1]!, 16);
  const a = m[2] ? Number.parseInt(m[2], 16) / 255 : 1;
  return `rgba(${(v >> 16) & 255}, ${(v >> 8) & 255}, ${v & 255}, ${Math.round(a * 1000) / 1000})`;
}

/** The colour of the row's swatch icon (`getColorIconItem`, drawn opaque by `drawBitmap`): copper, dielectric and paste a fixed colour each, a mask or a silkscreen the
 * colour its box has selected (`GetSelectedColor`). */
export function iconColorCss(l: StackupLayer, sub: number): string {
  const type = stackupItemType(l.name);
  if (type === "copper") return "rgb(220, 180, 30)";
  if (type === "dielectric") return "rgb(75, 120, 75)";
  if (type === "solderpaste") return "rgb(200, 200, 200)";
  const color = kicadPrms(l, sub).color;
  const idx = colorIndex(type, color);
  if (idx === standardColors(type).length - 1) {
    const css = hexColorCss(color);
    return css ? css.replace(/^rgba\((.*), [^,]+\)$/, "rgb($1)") : "rgb(128, 128, 128)";
  }
  const [r, g, b] = standardColors(type)[idx]!.rgb;
  return `rgb(${r}, ${g}, ${b})`;
}

// ------------------------------------------------------------------------------------------------------------------ the material lists

/** `DIELECTRIC_SUBSTRATE`: a material's name, epsilon R and loss tangent. */
export interface DielectricSubstrate {
  name: string;
  epsilonR: number;
  lossTangent: number;
}

/** `DIELECTRIC_SUBSTRATE_LIST::DL_MATERIAL_LIST_TYPE`. */
export type MaterialListType = "dielectric" | "soldermask" | "silkscreen";

/** `substrateMaterial`, `solderMaskMaterial`, `silkscreenMaterial` (dielectric_material.cpp): the lists a new panel starts with. */
const PREDEFINED_MATERIALS: Record<MaterialListType, readonly DielectricSubstrate[]> = {
  dielectric: [
    { name: NOT_SPECIFIED, epsilonR: 0.0, lossTangent: 0.0 },
    { name: "FR4", epsilonR: 4.5, lossTangent: 0.02 },
    { name: "FR408-HR", epsilonR: 3.69, lossTangent: 0.0091 },
    { name: "Polyimide", epsilonR: 3.2, lossTangent: 0.004 },
    { name: "Kapton", epsilonR: 3.2, lossTangent: 0.004 },
    { name: "Polyolefin", epsilonR: 1.0, lossTangent: 0.0 },
    { name: "Al", epsilonR: 8.7, lossTangent: 0.001 },
    { name: "PTFE", epsilonR: 2.1, lossTangent: 0.0002 },
    { name: "Teflon", epsilonR: 2.1, lossTangent: 0.0002 },
    { name: "Ceramic", epsilonR: 1.0, lossTangent: 0.0 },
  ],
  soldermask: [
    { name: NOT_SPECIFIED, epsilonR: DEFAULT_EPSILON_R_SOLDERMASK, lossTangent: 0.0 },
    { name: "Epoxy", epsilonR: DEFAULT_EPSILON_R_SOLDERMASK, lossTangent: 0.0 },
    { name: "Liquid Ink", epsilonR: DEFAULT_EPSILON_R_SOLDERMASK, lossTangent: 0.0 },
    { name: "Dry Film", epsilonR: DEFAULT_EPSILON_R_SOLDERMASK, lossTangent: 0.0 },
  ],
  silkscreen: [
    { name: NOT_SPECIFIED, epsilonR: DEFAULT_EPSILON_R_SILKSCREEN, lossTangent: 0.0 },
    { name: "Liquid Photo", epsilonR: DEFAULT_EPSILON_R_SILKSCREEN, lossTangent: 0.0 },
    { name: "Direct Printing", epsilonR: DEFAULT_EPSILON_R_SILKSCREEN, lossTangent: 0.0 },
  ],
};

/** The panel's three lists as it starts (`m_delectricMatList`, `m_solderMaskMatList`, `m_silkscreenMatList`): fresh copies, which the panel then adds to and the dialog
 * deletes from for as long as it is open. */
export function newMaterialLists(): Record<MaterialListType, DielectricSubstrate[]> {
  return { dielectric: [...PREDEFINED_MATERIALS.dielectric], soldermask: [...PREDEFINED_MATERIALS.soldermask], silkscreen: [...PREDEFINED_MATERIALS.silkscreen] };
}

/** The material list of a layer (`onMaterialChange`'s `item_mat_list`), or null for a layer with no material (`IsMaterialEditable`). */
export function materialListType(name: string): MaterialListType | null {
  const t = stackupItemType(name);
  return t === "dielectric" || t === "soldermask" || t === "silkscreen" ? t : null;
}

/** `DIELECTRIC_SUBSTRATE_LIST::FindSubstrate( aName, aEpsilonR, aLossTg )`: the entry of that name (any case) with exactly those values, else -1. */
export function findSubstrate(list: readonly DielectricSubstrate[], name: string, epsilonR: number, lossTangent: number): number {
  return list.findIndex((m) => m.epsilonR === epsilonR && m.lossTangent === lossTangent && m.name.toLowerCase() === name.toLowerCase());
}

/**
 * `onMaterialChange` before its dialog: "Ensure m_materialList contains all materials already in use in stackup list and add it is missing" -- every sublayer of
 * every dielectric, mask and silkscreen, with the material, epsilon R and loss tangent `transferDataFromUIToStackup` has just given it (an unspecified material is
 * "Not specified"), added to its kind's list when the list has no entry of that name and values. The lists are changed in place, as KiCad's members are.
 */
export function addMaterialsInUse(lists: Record<MaterialListType, DielectricSubstrate[]>, s: Stackup): void {
  for (const l of s.layers) {
    const type = materialListType(l.name);
    if (!type) continue;
    const list = lists[type];
    for (let sub = 0; sub < sublayerCount(l); sub++) {
      const p = kicadPrms(l, sub);
      const material = prmSpecified(p.material) ? p.material : NOT_SPECIFIED;
      if (findSubstrate(list, material, p.epsilonR, p.lossTangent) < 0) list.push({ name: material, epsilonR: p.epsilonR, lossTangent: p.lossTangent });
    }
  }
}

/** `DIALOG_DIELECTRIC_MATERIAL`'s words. */
export const MATERIAL_DIALOG = {
  title: "Dielectric Material Characteristics",
  name: "Name:",
  epsilonR: "Epsilon R:",
  lossTangent: "Loss Tan:",
  common: "Common materials:",
  columns: ["Material", "Epsilon R", "Loss Tan"],
  badEpsilonR: "Incorrect value for Epsilon R",
  badLossTangent: "Incorrect value for Loss Tangent",
} as const;

/** `DIALOG_DIELECTRIC_MATERIAL::TransferDataToWindow`: the fields start empty, "1" and "0" (a dummy substrate's `FormatEpsilonR` / `FormatLossTangent`). */
export function materialDialogStart(): { name: string; epsilonR: string; lossTangent: string } {
  return { name: "", epsilonR: formatPrm(1.0), lossTangent: formatPrm(0.0) };
}

/** `onListItemSelected`: the entry's name and values put in the fields. */
export function materialDialogPick(m: DielectricSubstrate): { name: string; epsilonR: string; lossTangent: string } {
  return { name: m.name, epsilonR: formatPrm(m.epsilonR), lossTangent: formatPrm(m.lossTangent) };
}

/** `DIALOG_DIELECTRIC_MATERIAL::TransferDataFromWindow` then `GetSelectedSubstrate`: the substrate, or the message that refuses OK ("Incorrect value for Epsilon R",
 * then "Incorrect value for Loss Tangent": not a number or negative). */
export function materialDialogResult(fields: { name: string; epsilonR: string; lossTangent: string }): { substrate: DielectricSubstrate } | { error: string } {
  const e = toDouble(fields.epsilonR);
  if (e === null || e < 0) return { error: MATERIAL_DIALOG.badEpsilonR };
  const t = toDouble(fields.lossTangent);
  if (t === null || t < 0) return { error: MATERIAL_DIALOG.badLossTangent };
  return { substrate: { name: fields.name, epsilonR: e, lossTangent: t } };
}

/**
 * `onMaterialChange` after its dialog, on sublayer `sub` of layer `layer`: an empty name changes nothing ("No substrate specified"); else the material, epsilon R and
 * loss tangent are the substrate's, and a dielectric whose colour is not user defined takes the colour of its material -- "PTFE natural" for PTFE or Teflon,
 * "Polyimide" for Polyimide or Kapton, "Aluminum" for Al, "FR4 natural" for anything else (names compared as written, `IsSameAs`).
 */
export function applySubstrate(s: Stackup, layer: number, sub: number, substrate: DielectricSubstrate): Stackup {
  if (substrate.name === "") return s;
  return {
    ...s,
    layers: s.layers.map((l, i) => {
      if (i !== layer) return l;
      const patch: Partial<StackupSublayer> = { material: substrate.name, epsilon_r: substrate.epsilonR, loss_tangent: substrate.lossTangent };
      if (stackupItemType(l.name) === "dielectric" && !kicadPrms(l, sub).color.startsWith("#")) {
        const n = substrate.name;
        patch.color = n === "PTFE" || n === "Teflon" ? "PTFE natural" : n === "Polyimide" || n === "Kapton" ? "Polyimide" : n === "Al" ? "Aluminum" : "FR4 natural";
      }
      return withSublayer(l, sub, patch);
    }),
  };
}

// ------------------------------------------------------------------------------------------------------------------ what Apply writes

/**
 * `transferDataFromUIToStackup`: each row's controls written back to its sublayer. A field always holds a number, so a blank epsilon R or loss tangent (this model's
 * "not written") is written as the number shown, the type's default; a material shown is written as shown (a dielectric's blank one is "FR4"); the colour is
 * the box's: a `#...` colour or a name of the list, and a name the list does not have is "Not specified" (left blank here) as the box shows it. The board file then
 * writes epsilon R and loss tangent wherever a material is specified, as `FormatBoardStackup` does.
 */
export function withKicadValues(s: Stackup): Stackup {
  return {
    ...s,
    layers: s.layers.map((l) => {
      let next = l;
      for (let sub = 0; sub < sublayerCount(l); sub++) {
        const p = kicadPrms(l, sub);
        const own = sublayerOf(l, sub);
        const patch: Partial<StackupSublayer> = {};
        if (itemHasEpsilonR(l.name)) {
          if (own.epsilon_r === null || own.epsilon_r === undefined) patch.epsilon_r = p.epsilonR;
          if (own.loss_tangent === null || own.loss_tangent === undefined) patch.loss_tangent = p.lossTangent;
        }
        if (itemMaterialEditable(l.name) && (own.material === null || own.material === undefined) && prmSpecified(p.material)) patch.material = p.material;
        const ct = colorListType(l.name);
        if (ct && typeof own.color === "string" && !own.color.startsWith("#") && colorIndex(ct, own.color) === 0) patch.color = undefined;
        if (Object.keys(patch).length > 0 || "color" in patch) next = withSublayer(next, sub, patch);
      }
      return next;
    }),
  };
}

// ------------------------------------------------------------------------------------------------------------------ Export to Clipboard

/**
 * `BuildStackupReport( m_stackup, m_frame->GetUserUnits() )`, word for word: a line for each layer (`layer "%s" type "%s"`, a dielectric's `\n  sublayer "1/%d"`
 * -- always, as `GetSublayersCount()` is never 0 -- its ` Color "%s"`, then for each sublayer ` Thickness %s` [` Locked`], ` Material "%s"`, ` EpsilonR %s`,
 * ` LossTg %s` as the type has them, a later sublayer starting `\n  sublayer "%d/%d"`), then `Finish "%s"` and the options. The values are the ones Apply would
 * write (`transferDataFromUIToStackup` runs first); `layerName` is `BOARD::GetLayerName` (`lazyBuildRowUI`'s `SetLayerName`).
 */
export function buildStackupReport(settings: Pick<StackupSettings, "stackup">, units: Units, layerName: (file: string) => string = (f) => f): string {
  const s = withKicadValues(settings.stackup);
  let report = "";
  let dielectrics = 0;
  for (const l of s.layers) {
    const type = stackupItemType(l.name);
    const count = sublayerCount(l);
    const typeName = l.kind ?? "";
    if (type === "dielectric") report += `layer "${formatDielectricLayerName(++dielectrics)}" type "${typeName}"\n  sublayer "1/${count}"`;
    else report += `layer "${layerName(l.name)}" type "${typeName}"`;
    if (colorListType(l.name)) {
      const c = kicadPrms(l, 0).color;
      report += ` Color "${c === "" ? NOT_SPECIFIED : c}"`;
    }
    for (let idx = 0; idx < count; idx++) {
      if (idx) report += `\n  sublayer "${idx + 1}/${count}"`;
      const p = kicadPrms(l, idx);
      if (itemThicknessEditable(l.name)) {
        report += ` Thickness ${stringFromValue(p.thicknessNm, units)}`;
        if (type === "dielectric" && p.thicknessLocked) report += " Locked";
      }
      // after `transferDataFromUIToStackup`, an unspecified material is the keyword `NotSpecifiedPrm()`
      if (itemMaterialEditable(l.name)) report += ` Material "${prmSpecified(p.material) ? p.material : NOT_SPECIFIED}"`;
      if (itemHasEpsilonR(l.name)) report += ` EpsilonR ${formatPrm(p.epsilonR)}`;
      if (itemHasEpsilonR(l.name)) report += ` LossTg ${formatPrm(p.lossTangent)}`;
    }
    report += "\n";
  }
  // `PANEL_SETUP_BOARD_FINISH::TransferDataFromWindow`: "Not specified" is the first entry of the finishes
  report += `Finish "${s.copper_finish ?? NOT_SPECIFIED}"`;
  if (s.dielectric_constraints) report += ' Option "Impedance Controlled"';
  if (s.edge_plating) report += ' Option "Plated edges"';
  if ((s.edge_connector ?? 0) !== 0) report += ` EdgeConnector "${s.edge_connector === 2 ? "yes,bevelled" : "yes"}"`;
  report += "\n";
  return report;
}

// ------------------------------------------------------------------------------------------------------------------ pads on a removed layer

/** `::FlipLayer( aLayerId, aCopperLayersCount )` of a copper layer (crates/model/src/padstack.rs `flip_copper_layer`): front and back swap, and from four copper layers
 * the inner ones mirror through the middle of the stack. */
export function flipCopperLayer(layer: string, copperLayers: number): string {
  if (layer === "F.Cu") return "B.Cu";
  if (layer === "B.Cu") return "F.Cu";
  const m = /^In(\d+)\.Cu$/.exec(layer);
  if (!m || copperLayers < 4) return layer;
  const max = copperLayers - 3;
  return `In${Math.min(Math.max(max - (Number(m[1]) - 1), 0), max) + 1}.Cu`;
}

/**
 * `PAD::HasExplicitDefinitionForLayer( layer )` (`PADSTACK::m_copperProps.count( aLayer ) > 0`) for a pad of a footprint on `side`: F.Cu always; a front/inner/back
 * padstack the layers the file gives it, B.Cu and `PADSTACK::INNER_LAYERS` (In1.Cu) for its inner shape; a custom one each layer it names. The model keeps them as for
 * a footprint on the top side, so a bottom footprint's are flipped, as `PADSTACK::FlipLayers` flips the keys (`INNER_LAYERS` stays In1.Cu).
 */
export function padHasExplicitLayer(pad: Pick<NonNullable<BoardState["parts"][number]["pads"]>[number], "stack">, side: Side | undefined, layer: string, copperLayers: number): boolean {
  if (layer === "F.Cu") return true;
  const stack = pad.stack;
  if (!stack) return false;
  const flip = (l: string) => (side === "bottom" ? flipCopperLayer(l, copperLayers) : l);
  if (stack.mode === "front_inner_back") return (layer === "In1.Cu" && !!stack.inner) || (!!stack.back && flip("B.Cu") === layer);
  if (stack.mode === "custom") return Object.keys(stack.layers ?? {}).some((k) => flip(k) === layer);
  return false;
}

/** `COLOR4D::ToHexString` (`#%02X%02X%02X%02X`, each channel `KiROUND( c * 255 )`) of a CSS `rgb(..)` / `rgba(..)` or `#...` colour, or null. */
export function colorToHexString(css: string): string | null {
  const s = css.trim();
  const fn = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(s);
  if (fn) {
    const ch = (v: string) => Math.min(255, Math.max(0, Math.round(Number(v))));
    return `#${hex2(ch(fn[1]!))}${hex2(ch(fn[2]!))}${hex2(ch(fn[3]!))}${hex2(Math.round(Math.min(1, Math.max(0, fn[4] === undefined ? 1 : Number(fn[4]))) * 255))}`;
  }
  const m = /^#([0-9a-fA-F]{6})([0-9a-fA-F]{2})?$/.exec(s);
  return m ? `#${m[1]!.toUpperCase()}${(m[2] ?? "FF").toUpperCase()}` : null;
}
