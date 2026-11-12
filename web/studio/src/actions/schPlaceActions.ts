// The schematic's placing tools with an item in the hand -- `SCH_DRAWING_TOOLS::TwoClickPlace` (Place Label, Global Label, Hierarchical Label, Directive
// Label, Text), `PlaceSymbol` (Place Symbol, A) and `PlaceSymbol` with the power filter (Place Power), eeschema/tools/sch_drawing_tools.cpp at 8303b2ad. The rules are
// kicad-port/schPlaceTool.ts; this is the glue: the tools' actions, the first click that opens the dialog or the chooser, the item that then rides the
// cursor (state/schCarryStore.ts, drawn by SchematicView), the click or the tool's hotkey that places it, and the keys the tool answers while it rides.
//
//   * The tool's action (L, Ctrl+L, H, T, P, Place Directive Label): with the tool not armed, it arms it and makes the item at once, where the cursor is
//     (`PrimeTool( aEvent.Position() )`, or "immediate actions" from a menu); with the item riding, it places it (`isSyntheticClick`); armed with nothing
//     carried, it leaves the tool.
//   * The first click with nothing carried makes the item: a dialog (`createNewLabel`, `createNewText`), or a local or global label on a wire a label names
//     takes that name with no dialog ("Auto-create from attached wire"), or the chooser of power symbols (`PickSymbolFromLibrary( &filter, ... )`).
//   * The click with an item carried places it with the label, text, power or symbol verb the dialogs always sent -- one `/api/cmd` step, one undo -- and
//     the tool holds the next label of a multi-line entry, the same power symbol again with "Place repeated copies", the next unit with "Place all
//     units" or a copy with the next reference with "Place repeated copies" (`keepSymbol`, `placeAllUnits`), or nothing; a symbol an action handed the
//     tool (`placeOneOnly`) ends it.
import type { Dispatch } from "react";
import type { Action, StudioApi, StudioState } from "../state/store";
import actionsData from "../kicad/actions.json";
import type { ActionsFile } from "../kicad/types";
import { fetchSchematic, postPlacePreview } from "../api/client";
import type { Cmd, LibSymbol, Schematic } from "../api/types";
import { inferSpin } from "../components/schematic/labelShape";
import { labelNameFromWire } from "../kicad-port/schLabelName";
import { lastPlaced } from "../kicad-port/schLastPlaced";
import { rememberedAnnotateSettings, startNumber } from "../kicad-port/schAnnotate";
import { fetchPlacementReference } from "../api/hierarchyClient";
import { fetchSymbolPlacements } from "../api/schControlClient";
import { afterPlaced, applyCarriedCmds, armedKey, carryKey, incrementCarried, isCarryTool, mirrorCarried, placeCmd, previewCmd, renameInCmd, rotateCarried, scopeOfTool, settledSymbol, type CarriedItem, type CarriedSymbol, type CarryTool, unitOccupied, unitsByReference } from "../kicad-port/schPlaceTool";
import { needsServerDrawing } from "../kicad-port/schCarryPreview";
import { bodyStyleCount } from "../kicad-port/schBodyStyle";
import { editFieldTarget } from "../kicad-port/schFieldEdit";
import type { SymbolAttrKey } from "../kicad-port/schControl";
import { nextReference, placementMinRef } from "../kicad-port/nextReference";
import { unitCountOf } from "../kicad-port/schUnits";
import { carryCursor, getSchCarry, noteCarryMade, noteCarryRequest, noteChooserAt, schematicWithCarry, setSchCarry, type SchCarry } from "../state/schCarryStore";
import { schematicActions, type ActionMap } from "./schActionRegistry";

export interface PlaceContext {
  state: StudioState;
  dispatch: Dispatch<Action>;
  api: StudioApi;
  /** The cursor, grid-snapped, or null when the pointer is off the canvas. */
  cursorSnapped: () => [number, number] | null;
}

/** The actions with `AF_ACTIVATE`: each starts a tool (`TOOL_EVENT::IsActivate`). */
const ACTIVATES: ReadonlySet<string> = new Set((actionsData as ActionsFile).actions.filter((a) => (a.flags ?? []).includes("AF_ACTIVATE")).map((a) => a.name));

let seq = 0;
/** The state of the carried item the server's last kept drawing is of (a drawing of an older state that comes back after a newer one is dropped). */
let serverSeq = -1;

/**
 * Ask the server for its drawing of `carry`'s item, for what only it knows (kicad-port/schCarryPreview.ts `needsServerDrawing`: a symbol's fields, a global
 * label's Sheet References, a text's variables). The page draws the item meanwhile; the drawing is kept, as of the state it was asked for, while the tool holds
 * an item, and laid over the page's own when it is of this very state.
 */
function fetchPreview(carry: SchCarry): void {
  if (!needsServerDrawing(carry.item)) return;
  noteCarryRequest();
  const item = carry.item;
  const asked = carry.seq;
  void postPlacePreview({ sheet: carry.sheet.join("/"), cmd: previewCmd(item, powerIdOf(carry)) }).then((preview) => {
    const now = getSchCarry();
    if (!preview || !now || asked < serverSeq || now.tool !== carry.tool) return;
    serverSeq = asked;
    setSchCarry({ ...now, server: { item, preview } });
  });
}

/** The id the server numbers the carried power symbol with: its drawing's (the same at the preview's point and the click's). */
function powerIdOf(carry: SchCarry): string | undefined {
  return carry.item.kind === "power" ? carry.server?.preview.power_symbols[0]?.id : undefined;
}

/**
 * The tool now holds `item` (`prepItemForPlacement` / `addSymbol`): it rides the cursor, drawn by the page at once. `lib` is a carried symbol's library drawing
 * when the sheet has none (the chooser's).
 */
export function carryItem(ctx: Pick<PlaceContext, "api">, tool: CarryTool, item: CarriedItem, at: [number, number], queue: CarriedItem[] = [], keep = false, lib: LibSymbol | null = null): void {
  const st = ctx.api.getState();
  serverSeq = -1;
  const carry: SchCarry = { tool, item, queue, keep, sheet: [...st.currentSheetPath], at, lib, server: null, seq: ++seq };
  noteCarryMade();
  setSchCarry(carry);
  fetchPreview(carry);
}

/** The carried item changed (turned, mirrored, edited, incremented): the page draws it anew at once; the server's drawing of it follows where it is needed. */
export function changeCarried(item: CarriedItem): void {
  const now = getSchCarry();
  if (!now) return;
  const next: SchCarry = { ...now, item, seq: ++seq, editing: false };
  noteCarryMade();
  setSchCarry(next);
  fetchPreview(next);
}

/**
 * The first click with nothing carried (`PLACE_NEXT`: "First click creates..."): the item's dialog or chooser opens at `at`; a local or global label
 * on a wire a label already names is made with that name, with the tool's last spin, style, shape and Auto, and rides at once.
 */
export function beginItem(ctx: Pick<PlaceContext, "api" | "dispatch">, tool: CarryTool, at: [number, number]): void {
  const { dispatch } = ctx;
  const sch = ctx.api.getState().schematic;
  if (!sch) return;
  dispatch({ type: "SET_SELECTION", refs: [] }); // `ACTIONS::selectionClear`
  const scope = scopeOfTool(tool);
  if (scope) {
    const named = labelNameFromWire(sch.wires, at, scope);
    if (named) {
      const last = lastPlaced();
      carryItem(ctx, tool, { kind: "label", scope, text: named, shape: last.shape, spin: last.spin, sizeNm: null, bold: last.bold, italic: last.italic, autoRotate: scope !== "local" && last.autoRotate }, at);
      return;
    }
    dispatch({ type: "SET_SCH_LABEL_PENDING", pending: { at, scope } });
    return;
  }
  if (tool === "sch_text") dispatch({ type: "SET_SCH_TEXT_PENDING", pending: { at } });
  else if (tool === "sch_directive") dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "directive", at: { x: at[0], y: at[1] } } });
  else if (tool === "sch_place_symbol") {
    // `PlaceSymbol`'s first click with no symbol in hand: `PickSymbolFromLibrary( &filter, m_symbolHistoryList, alreadyPlaced, .. )`; the symbol chosen rides from
    // where the click was ("If we started with a hotkey which has a position then warp back to that")
    noteChooserAt(at);
    dispatch({ type: "SET_SYMBOL_CHOOSER_OPEN", open: true });
  } else dispatch({ type: "SET_SCH_POWER_PENDING", pending: { at } });
}

/**
 * The reference a new symbol of `prefix` rides with (`PlaceSymbol`'s `annotate()`: `ReannotateByOptions` with the project's Annotate method and start number), the
 * next free one on the sheet and the board; `taken` adds references the sheet has not shown yet (the one a click has just placed).
 */
export function newSymbolRef(ctx: Pick<PlaceContext, "api">, prefix: string, taken: readonly string[] = []): string {
  const st = ctx.api.getState();
  const a = rememberedAnnotateSettings();
  return nextReference(st.schematic?.symbols ?? [], prefix || "U", [...(st.board?.parts.map((p) => p.ref) ?? []), ...taken], placementMinRef(a.algo, startNumber(a.start), st.schematic?.page_number ?? 1));
}

/**
 * `PlaceSymbol`'s `annotate()` asked of the server (`ReannotateByOptions` over every reference of the hierarchy, which the page does not hold): the carried symbol `item`
 * takes the reference the server gives, if it is still in the hand as it was. The page's own guess (`newSymbolRef`, this sheet's symbols and the board's parts) rides
 * until then; the server's also joins a unit to a part of the same symbol and value that has the unit free (r2 D-A1, UI-LEFT S14).
 */
function refineCarriedReference(ctx: Pick<PlaceContext, "api">, item: CarriedSymbol): void {
  const st = ctx.api.getState();
  const a = rememberedAnnotateSettings();
  void fetchPlacementReference({ sheet: st.currentSheetPath, prefix: item.prefix, lib_id: item.libId, value: item.value, unit: item.unit, unit_count: item.unitCount, options: { algo: a.algo, start_number: startNumber(a.start) } }).then((reference) => {
    const now = getSchCarry();
    if (!reference || !now || now.item.kind !== "symbol") return;
    const held = now.item;
    if (held.libId !== item.libId || held.ref !== item.ref || held.unit !== item.unit || reference === held.ref) return;
    // what the dialogs did to it named it by the reference it rode with
    changeCarried({ ...held, ref: reference, ...(held.edits ? { edits: held.edits.map((c) => renameInCmd(c, held.ref, reference)) } : {}) });
  });
}

/** What the chooser picked or an action handed the tool (`PICKED_SYMBOL`, `PLACE_SYMBOL_PARAMS`): the symbol `addSymbol` puts in the hand. */
export interface SymbolToCarry {
  libId: string;
  referencePrefix: string;
  /** A further unit of a placed reference goes in under it (Place Next Symbol Unit); otherwise the symbol is numbered (`annotate()`). */
  ref?: string;
  value?: string;
  footprint?: string;
  unit?: number;
  unitCount?: number;
  /** The body style chosen (KiCad 25ec2335's chooser "Body style:", `PICKED_SYMBOL::BodyStyle`); 1 when none. */
  bodyStyle?: number;
  rotDeg?: number;
  mirror?: "x" | "y" | null;
  placeAllUnits?: boolean;
  /** `placeOneOnly`: a symbol the action was handed; the tool ends after it is placed. */
  oneOnly?: boolean;
  /** Its library drawing, when the sheet has none (the chooser's): the page draws it in the hand from this. */
  lib?: LibSymbol | null;
}

/** `addSymbol( symbol )`: the symbol rides the cursor, drawn as the server draws its `add_symbol` (`POST /api/sch/place_preview`). */
export function carrySymbol(ctx: Pick<PlaceContext, "api">, pick: SymbolToCarry, at: [number, number], keep: boolean): void {
  const st = ctx.api.getState();
  const value = pick.value || (pick.libId.includes(":") ? pick.libId.slice(pick.libId.indexOf(":") + 1) : pick.libId);
  const unitCount = Math.max(1, pick.unitCount ?? unitCountOf(st.schematic?.lib_symbols[pick.libId]));
  const item: CarriedSymbol = {
    kind: "symbol",
    libId: pick.libId,
    ref: pick.ref ?? newSymbolRef(ctx, pick.referencePrefix),
    prefix: pick.referencePrefix || "U",
    value,
    footprint: pick.footprint ?? "",
    unit: Math.min(Math.max(pick.unit ?? 1, 1), unitCount),
    unitCount,
    rotDeg: pick.rotDeg ?? 0,
    mirror: pick.mirror ?? null,
    turns: [],
    placeAllUnits: pick.placeAllUnits ?? false,
    oneOnly: pick.oneOnly ?? false,
    ...(pick.bodyStyle && pick.bodyStyle > 1 ? { bodyStyle: pick.bodyStyle } : {}),
  };
  carryItem(ctx, "sch_place_symbol", item, at, [], keep, pick.lib ?? null);
  if (!pick.ref) refineCarriedReference(ctx, item);
  if (item.placeAllUnits && item.unitCount > 1) void refreshHierarchyUnits();
}

/**
 * The units of every reference placed over the hierarchy (`GET /api/sch/symbols`), which "Place all units" steps past (`unitOccupied`: `schematic.Contains`, every
 * sheet, UI-LEFT-2 SS17). Asked when such a symbol starts riding and after each one placed; until the answer the sheet in view's symbols decide.
 */
let hierarchyUnits: Map<string, Set<number>> = new Map();

async function refreshHierarchyUnits(): Promise<void> {
  try {
    hierarchyUnits = unitsByReference(await fetchSymbolPlacements());
  } catch {
    // the sheet in view's symbols still decide
  }
}

/** The second click (or the tool's hotkey): the carried item goes on the sheet at `at`, and the tool holds what comes next. */
export function placeCarried(ctx: Pick<PlaceContext, "api"> & Partial<Pick<PlaceContext, "dispatch">>, at: [number, number]): void {
  const carry = getSchCarry();
  const sch = ctx.api.getState().schematic;
  if (!carry || !sch) return;
  const plainSpin = carry.item.kind === "label" && !carry.item.spin ? inferSpin(sch.wires, at) : null;
  // `PlaceSymbol` numbers a power symbol by the project's Annotate method and start number (r3 D3-M5)
  const a = rememberedAnnotateSettings();
  const committed = ctx.api.cmd(placeCmd(carry.item, at, { plainSpin, annotate: { algo: a.algo, start_number: startNumber(a.start) }, powerId: powerIdOf(carry) }));
  const placed = carry.item;
  // the units of the reference already on the sheet, the one just placed among them (`unitOccupied`), and the next free reference past it (`annotate()`)
  const next = afterPlaced(
    carry,
    placed.kind === "symbol"
      ? { occupied: (u) => unitOccupied(placed.ref, u, placed, sch.symbols, hierarchyUnits), nextRef: () => newSymbolRef(ctx, placed.prefix, [placed.ref]) }
      : undefined
  );
  // the hierarchy's units once this one is in, for the next click of "Place all units"
  if (placed.kind === "symbol" && placed.placeAllUnits) void committed.then(() => refreshHierarchyUnits());
  if (next) {
    // the next one rides, selected in its place (`addSymbol` / `prepItemForPlacement`: `ClearSelection(); AddItemToSel( item )`)
    ctx.dispatch?.({ type: "SET_SELECTION", refs: [] });
    carryItem(ctx, next.tool, next.item, at, next.queue, next.keep, carry.lib ?? null);
    // a new reference for the next symbol: the server's, once the one just placed is in (the next unit of the same reference keeps it)
    const nextItem = next.item;
    if (nextItem.kind === "symbol" && placed.kind === "symbol" && nextItem.ref !== placed.ref) void committed.then(() => refineCarriedReference(ctx, nextItem));
  } else {
    setSchCarry(null);
    selectPlaced(ctx, sch, carry.sheet, placed, committed);
  }
  // `if( placeOneOnly ) { m_frame->PopTool( aEvent ); break; }`
  if (placed.kind === "symbol" && placed.oneOnly) ctx.dispatch?.({ type: "SET_ACTIVE_TOOL", tool: "select" });
}

let placing = 0;

/**
 * The item just placed is left selected: the tool had it in the selection while it rode (`addSymbol`'s and `prepItemForPlacement`'s `AddItemToSel( item )`), and
 * the click that commits it clears nothing (`commit.Push`, then `item = nullptr` / `symbol = nextSymbol`). The new item is read off the sheet once the command is
 * in -- unless the tool holds something again by then, or another placement followed.
 */
function selectPlaced(ctx: Pick<PlaceContext, "api"> & Partial<Pick<PlaceContext, "dispatch">>, before: Schematic, sheet: readonly string[], placed: CarriedItem, committed: Promise<boolean>): void {
  const mine = ++placing;
  void committed.then(async (ok) => {
    if (!ok || mine !== placing || getSchCarry()) return;
    const after = await fetchSchematic(sheet).catch(() => null);
    if (!after || mine !== placing || getSchCarry() || ctx.api.getState().currentSheetPath.join("/") !== sheet.join("/")) return;
    const ids = placedIds(before, after, placed);
    if (ids.length > 0) ctx.dispatch?.({ type: "SET_SELECTION", refs: ids });
  });
}

/** What a placing click added of `placed`'s kind (the label, text, directive label, power symbol or symbol; not a junction or a wire it cut). */
export function placedIds(before: Schematic, after: Schematic, placed: CarriedItem): string[] {
  const added = <T extends { id: string }>(was: readonly T[] | undefined, now: readonly T[] | undefined): string[] => {
    const old = new Set((was ?? []).map((x) => x.id));
    return (now ?? []).filter((x) => !old.has(x.id)).map((x) => x.id);
  };
  switch (placed.kind) {
    case "label":
      return added(before.labels, after.labels);
    case "text":
      return added(before.texts, after.texts);
    case "directive":
      return added(before.graphics, after.graphics);
    case "power":
      return added(before.power_symbols, after.power_symbols);
    case "symbol":
      return after.symbols.some((x) => x.id === placed.ref && x.unit === placed.unit) && !before.symbols.some((x) => x.id === placed.ref && x.unit === placed.unit) ? [placed.ref] : [];
  }
}

/**
 * Where the cursor is: with `snapped`, the point the armed tool's grid helper last put it at (`BestSnapAnchor`, the loop's `cursorPos`); else the grid point
 * nearest it (`grid.Align( evt->Position() )`, where a hotkey primes the tool); else the middle of the view.
 */
function cursorAt(ctx: PlaceContext, snapped = false): [number, number] {
  const c = (snapped ? carryCursor() : null) ?? ctx.cursorSnapped();
  if (c) return c;
  const v = ctx.state.schematicView;
  const scale = v.scale || 1;
  const el = typeof document === "undefined" ? null : document.querySelector(".pcb-canvas-container");
  return [Math.round(((el?.clientWidth ?? 800) / 2 - v.x) / scale), Math.round(((el?.clientHeight ?? 600) / 2 - v.y) / scale)];
}

/** A placing tool's action: arm the tool and make its item where the cursor is, place the item it carries, or leave the tool. */
function armPlacingTool(ctx: PlaceContext, tool: CarryTool): void {
  const { state, dispatch } = ctx;
  if (state.tab !== "schematic") return;
  if (state.activeTool === tool) {
    if (getSchCarry()) placeCarried(ctx, cursorAt(ctx, true));
    else dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
    return;
  }
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  dispatch({ type: "SET_ACTIVE_TOOL", tool });
  beginItem(ctx, tool, cursorAt(ctx));
}

/** The id the carried item has in the page's drawing of it (`schematicWithCarry`): a symbol's reference, a power symbol's number once the server gave it. */
function carriedIdOf(carry: SchCarry): string | null {
  return carry.item.kind === "symbol" ? carry.item.ref : carry.item.kind === "power" ? (powerIdOf(carry) ?? null) : null;
}

/**
 * Properties of the carried item (`SCH_EDIT_TOOL::Properties` on the selection, which is the item): its dialog opens on it, and OK changes it -- Symbol
 * Properties on a carried symbol, the power symbol's Properties on a carried power symbol (once the server has numbered it), a label's, a text's or a directive
 * label's dialog on those.
 */
function reopenDialog(ctx: PlaceContext, carry: SchCarry): void {
  const at = cursorAt(ctx);
  const item = carry.item;
  if (item.kind === "symbol") {
    ctx.dispatch({ type: "SET_SYMBOL_PROPERTIES", value: { id: item.ref, unit: item.unit, field: null, carried: true } });
    return;
  }
  if (item.kind === "power") {
    const id = powerIdOf(carry);
    if (!id) return;
    ctx.dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_power", id, carried: true } });
    return;
  }
  setSchCarry({ ...carry, editing: true });
  if (item.kind === "label") ctx.dispatch({ type: "SET_SCH_LABEL_PENDING", pending: { at, scope: item.scope } });
  else if (item.kind === "text") ctx.dispatch({ type: "SET_SCH_TEXT_PENDING", pending: { at } });
  else ctx.dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "directive", at: { x: at[0], y: at[1] } } });
}

/**
 * U, V, F with a symbol or power symbol carried (`SCH_EDIT_TOOL::EditField` on the selection): Field Properties on that field of it, as the page draws it with the
 * server's fields; a symbol whose fields have not come back yet opens Symbol Properties at the field (`editFieldTarget`'s fallback).
 */
function editCarriedField(ctx: PlaceContext, carry: SchCarry, name: "Reference" | "Value" | "Footprint"): void {
  const sch = ctx.api.getState().schematic;
  const id = carriedIdOf(carry);
  if (!sch || !id) return;
  const target = editFieldTarget(schematicWithCarry(sch, carry), [id], name, carry.item.kind === "symbol" ? carry.item.unit : undefined);
  if (target.kind === "none") return;
  if (target.kind === "field") ctx.dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "props_field", id: target.id, carried: true } });
  else if (carry.item.kind === "symbol") ctx.dispatch({ type: "SET_SYMBOL_PROPERTIES", value: { id: carry.item.ref, unit: carry.item.unit, field: name === "Reference" ? "reference" : name === "Value" ? "value" : "footprint", carried: true } });
}

/** Cycle Body Style on the symbol in the hand (`SCH_EDIT_TOOL::CycleBodyStyle`): the next body style of its library symbol, back to the first past the last. */
function cycleCarriedBodyStyle(ctx: PlaceContext, carry: SchCarry): void {
  const item = carry.item;
  const sch = ctx.api.getState().schematic;
  if (item.kind !== "symbol" || !sch) return;
  const view = schematicWithCarry(sch, carry);
  const count = bodyStyleCount(view, { lib_id: item.libId });
  if (count <= 1) return void ctx.dispatch({ type: "TOAST", message: "Select a symbol that has more than one body style.", kind: "info" });
  changeCarried({ ...item, bodyStyle: ((item.bodyStyle ?? 1) % count) + 1 });
}

/**
 * An attribute toggle with a symbol or power symbol in the hand (`SCH_EDIT_TOOL::SetAttribute` on the selection, which is the one item: `new_state` is on when it
 * lacks the attribute). A carried symbol keeps the flags it is made with; a power symbol's are its Properties' (`set_power_props`, by the id the server numbers it with).
 */
function toggleCarriedAttribute(carry: SchCarry, key: SymbolAttrKey): void {
  const item = carry.item;
  if (item.kind === "symbol") return changeCarried({ ...item, attrs: { ...item.attrs, [key]: !item.attrs?.[key] } });
  if (item.kind !== "power") return;
  const id = powerIdOf(carry);
  if (!id) return;
  const props = carry.server?.preview.power_symbols[0]?.props;
  const now = !!(props as Partial<Record<SymbolAttrKey, boolean>> | null | undefined)?.[key];
  changeCarried({ ...item, edits: [...(item.edits ?? []), { op: "sch_edit", verb: "set_power_props", id, [key]: !now } as unknown as Cmd] });
}

/**
 * OK in a dialog opened on the carried symbol or power symbol (Symbol Properties, a power symbol's Properties, Field Properties): the dialog's commands change the
 * item in the hand (kicad-port/schPlaceTool.ts `applyCarriedCmds`) instead of the sheet. False when the tool holds no such item any more.
 */
export function editCarried(cmds: readonly Cmd[]): boolean {
  const carry = getSchCarry();
  if (!carry || (carry.item.kind !== "symbol" && carry.item.kind !== "power")) return false;
  if (cmds.length > 0) changeCarried(applyCarriedCmds(carry.item, cmds));
  return true;
}

/** Esc, Undo or Delete with an item carried: it goes and the tool stays. Place Symbol and Place Power with "Place repeated copies" open the chooser again ("Re-enter symbol chooser"). */
function dropCarried(ctx: PlaceContext, carry: SchCarry): void {
  setSchCarry(null);
  if (carry.tool === "sch_power" && carry.keep) ctx.dispatch({ type: "SET_SCH_POWER_PENDING", pending: { at: cursorAt(ctx) } });
  if (carry.tool === "sch_place_symbol" && carry.keep) {
    noteChooserAt(cursorAt(ctx));
    ctx.dispatch({ type: "SET_SYMBOL_CHOOSER_OPEN", open: true });
  }
}

/**
 * The placing tools' actions, and every other action as the tools' loops answer it while an item rides (`carryKey`). Registered last: it wraps what
 * every other registration put in the map.
 */
export function registerSchPlaceActions(registry: ActionMap, ctx: PlaceContext): void {
  // (one by one: `CARRY_TOOL_ACTIONS` names the same pairs; another tab keeps what it registered under the name)
  const m = schematicActions(registry, ctx.state.tab);
  m.set("eeschema.InteractiveDrawing.placeLabel", () => armPlacingTool(ctx, "sch_label_local"));
  m.set("eeschema.InteractiveDrawing.placeGlobalLabel", () => armPlacingTool(ctx, "sch_label_global"));
  m.set("eeschema.InteractiveDrawing.placeHierarchicalLabel", () => armPlacingTool(ctx, "sch_label_hier"));
  m.set("eeschema.InteractiveDrawing.placeClassLabel", () => armPlacingTool(ctx, "sch_directive"));
  m.set("eeschema.InteractiveDrawing.placeSchematicText", () => armPlacingTool(ctx, "sch_text"));
  m.set("eeschema.InteractiveDrawing.placePowerSymbol", () => armPlacingTool(ctx, "sch_power"));
  // Place Symbol (A): `PlaceSymbol` with `m_symbolHistoryList` -- the chooser at the first click, then the symbol rides (`addSymbol`)
  m.set("eeschema.InteractiveDrawing.placeSymbol", () => armPlacingTool(ctx, "sch_place_symbol"));
  if (ctx.state.tab !== "schematic") return;
  for (const [id, run] of [...registry]) {
    registry.set(id, (param) => {
      const carry = getSchCarry();
      // armed with nothing in the hand: `TwoClickPlace` leaves on Undo
      if (!carry && isCarryTool(ctx.state.activeTool) && armedKey(ctx.state.activeTool, id) === "leave") return void ctx.dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      // an item made on this sheet with this tool; anything else is not the tool's to answer
      if (!carry || ctx.state.activeTool !== carry.tool) return run(param);
      const k = carryKey(carry.tool, id, ACTIVATES.has(id));
      switch (k.do) {
        case "pass":
          return run(param);
        case "place":
          return placeCarried(ctx, cursorAt(ctx, true));
        case "rotate":
          return changeCarried(rotateCarried(carry.item, k.ccw));
        case "mirror":
          return changeCarried(mirrorCarried(carry.item, k.vertical));
        case "increment": {
          const next = incrementCarried(carry.item, k.delta, k.index);
          if (next) changeCarried(next);
          return;
        }
        case "properties":
          return reopenDialog(ctx, carry);
        case "edit_field":
          return editCarriedField(ctx, carry, k.name);
        case "autoplace":
          // `AutoplaceFields( screen, AUTOPLACE_MANUAL )` on the symbol in the hand: its fields where Autoplace Fields puts them, whatever its turns left
          if (carry.item.kind === "symbol" && carry.item.turns.length > 0) changeCarried(settledSymbol(carry.item));
          return;
        case "body_style":
          return cycleCarriedBodyStyle(ctx, carry);
        case "attribute":
          return toggleCarriedAttribute(carry, k.key);
        case "unit": {
          // "<placed unit>:<unit chosen>" (kicad-port/schSymbolMenus.ts): `m_frame->SelectUnit( symbol, unit )` on the symbol in the hand
          const unit = Number(String(param ?? "").split(":")[1]);
          if (carry.item.kind === "symbol" && Number.isInteger(unit) && unit >= 1 && unit <= carry.item.unitCount && unit !== carry.item.unit) changeCarried({ ...carry.item, unit });
          return;
        }
        case "drop":
          return dropCarried(ctx, carry);
        case "busy":
          // `m_frame->ShowInfoBarMsg( _( "Press <ESC> to cancel item creation." ) )`
          ctx.dispatch({ type: "TOAST", message: k.message, kind: "info" });
          return;
        case "refuse":
          return;
      }
    });
  }
}
