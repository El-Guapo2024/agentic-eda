// Board Setup > Board Stackup > Physical Stackup, the parts of pcbnew/board_stackup_manager/panel_board_stackup.cpp that are pure: the rows the panel builds
// (`buildLayerStackPanel`, `lazyBuildRowUI`: one row per sublayer of a dielectric), Add Dielectric Layer and Remove Dielectric Layer (`onAddDielectricLayer`,
// `onRemoveDielectricLayer`, `onRemoveDielUI`, `BOARD_STACKUP_ITEM::AddDielectricPrms` / `RemoveDielectricPrms`), Adjust Dielectric Thickness
// (`onAdjustDielectricThickness`, `setDefaultLayerWidths`), the checks of `transferDataFromUIToStackup`, and what the board would lose with fewer copper layers
// (`PANEL_SETUP_LAYERS::TransferDataFromWindow`'s `getRemovedLayersWithItems` and the items it lists, `GetItemDescription( frame, true )`). The page is
// components/boardSetup/StackupPage.tsx; the backend stores the stackup (`set_stackup`, crates/ops/src/board_setup.rs) and deletes the items when told to.
//
// A layer's own `material`, `thickness_mm`, `thickness_locked`, `epsilon_r`, `loss_tangent` and `color` are its first sublayer; `sublayers` are the ones after it
// (crates/model/src/lib.rs `StackupLayer::sublayers`). Dependency-free (types only) so node --test runs it.
import type { BoardState, Nm, Stackup, StackupLayer, StackupSublayer } from "../api/types";
import { messageTextFromValue } from "./messageText";
import { stringFromValue } from "./fpFields";

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
export type RemovedLayerBoard = Pick<BoardState, "routing" | "drawings">;

/**
 * Fewer copper layers: `PANEL_SETUP_LAYERS::TransferDataFromWindow`'s list of what is on the layers that go, each as `GetItemDescription( frame, true )` says it.
 * The layers are those of `getRemovedLayersWithItems` -- the ones that go and have something on them (`BOARD::HasItemsOnLayer`: a through via does not count, nor
 * a blind or buried via that only passes through) -- in the board's order; for each, the board-level items on it (`PCB_LAYER_COLLECTOR` over `BoardLevelItems`,
 * `IsOnLayer`), so a via is listed for every layer its drill spans, a through via too, and a zone on several layers for each that goes. The order of a layer's
 * items is `BOARD::Visit`'s: the drawings (shapes, texts, text boxes, tables, dimensions, barcodes), the tracks and vias, the points, the zones, the generators.
 * Footprints' pads with a shape of their own on the layer, which KiCad lists too ("Pad %s of %s on %s"), are not: the model has no per-layer pad shapes on inner
 * layers.
 */
export function itemsOnRemovedLayers(board: RemovedLayerBoard | null | undefined, fromLayers: number, toLayers: number, units: Units = "mm", layerName: (file: string) => string = (f) => f): string[] {
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
  const out: string[] = [];
  for (const layer of stack.filter((l) => !keep.has(l) && hasItems(l))) {
    const on = layerName(layer);
    for (const s of dr?.shapes ?? []) if (s.layer === layer) out.push(`${SHAPE_NAMES[s.kind] ?? "Unrecognized"} ${netnameMsg(s.net)} on ${on}`);
    for (const t of dr?.texts ?? []) if (t.layer === layer) out.push(`PCB text '${t.shown ?? t.content}' on ${on}`);
    for (const t of dr?.text_boxes ?? []) if (t.layer === layer) out.push(`PCB text box '${t.shown ?? t.text}' on ${on}`);
    for (const t of dr?.tables ?? []) if (t.layer === layer) out.push(`${t.cols} column table`);
    for (const d of dr?.dimensions ?? []) if (d.layer === layer) out.push(`Dimension '${d.text}' on ${on}`);
    for (const b of dr?.barcodes ?? []) if (b.layer === layer) out.push(`Barcode on ${on}`);
    for (const t of rt?.tracks ?? []) {
      if (t.layer !== layer) continue;
      if (t.arc_mid && t.pts.length >= 2) {
        out.push(`Track (arc) ${netnameMsg(t.net)} on ${on}, length ${messageTextFromValue(arcLength(t.pts[0]!, t.arc_mid, t.pts[t.pts.length - 1]!), units)}`);
        continue;
      }
      for (let i = 0; i + 1 < t.pts.length; i++) {
        const [a, b] = [t.pts[i]!, t.pts[i + 1]!];
        out.push(`Track ${netnameMsg(t.net)} on ${on}, length ${messageTextFromValue(Math.hypot(b[0] - a[0], b[1] - a[1]), units)}`);
      }
    }
    for (const v of rt?.vias ?? []) {
      if (!viaOn(v, layer)) continue;
      const word = v.type === "blind" ? "Blind via" : v.type === "buried" ? "Buried via" : v.type === "micro" ? "Micro via" : "Via";
      // `PCB_VIA::LayerMaskDescribe`: the top and the bottom layer of `LayerPair`
      const [top, bottom] = viaSpan(v);
      out.push(`${word} ${netnameMsg(v.net)} on ${layerName(top)} - ${layerName(bottom)}`);
    }
    for (const p of dr?.points ?? []) if (p.layer === layer) out.push(`Point on ${on}`);
    for (const z of rt?.zones ?? []) {
      const set = zoneSet(z);
      if (!set.includes(layer)) continue;
      const desc = zoneLayerDesc(set, layerName);
      const name = z.name ?? "";
      if (z.is_rule_area) out.push(name === "" ? `Rule Area ${desc}` : `Rule area '${name}' ${desc}`);
      else if (z.teardrop) out.push(`Teardrop ${netnameMsg(z.net)} ${desc}`);
      else out.push(name === "" ? `Zone ${netnameMsg(z.net)} ${desc}, priority ${z.priority}` : `Zone '${name}' ${netnameMsg(z.net)} ${desc}, priority ${z.priority}`);
    }
    // `PCB_GENERATOR::GetItemDescription`: "Generator"
    for (const p of dr?.tuning_patterns ?? []) if (p.layer === layer) out.push("Generator");
  }
  return out;
}

/** The text of KiCad's question (`DIALOG_ITEMS_LIST( parent, _( "Warning" ), .., _( "Show Details" ) )`). */
export const REMOVED_LAYERS_TITLE = "Warning";
export const REMOVED_LAYERS_MESSAGE = "Items have been found on removed layers. This operation will delete all items from removed layers and cannot be undone.";
export const REMOVED_LAYERS_DETAILS = "Show Details";
