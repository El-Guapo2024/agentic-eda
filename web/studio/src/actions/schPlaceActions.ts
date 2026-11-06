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
import { postPlacePreview } from "../api/client";
import { inferSpin } from "../components/schematic/labelShape";
import { labelNameFromWire } from "../kicad-port/schLabelName";
import { lastPlaced } from "../kicad-port/schLastPlaced";
import { rememberedAnnotateSettings, startNumber } from "../kicad-port/schAnnotate";
import { afterPlaced, armedKey, carryKey, incrementCarried, isCarryTool, mirrorCarried, placeCmd, previewCmd, rotateCarried, scopeOfTool, type CarriedItem, type CarriedSymbol, type CarryTool } from "../kicad-port/schPlaceTool";
import { nextReference, placementMinRef } from "../kicad-port/nextReference";
import { unitCountOf } from "../kicad-port/schUnits";
import { carryCursor, getSchCarry, noteChooserAt, setSchCarry, type SchCarry } from "../state/schCarryStore";
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

/** Ask the server for the drawing of `carry`'s item; it is kept only if the item has not changed since. */
function fetchPreview(carry: SchCarry): void {
  void postPlacePreview({ sheet: carry.sheet.join("/"), cmd: previewCmd(carry.item) }).then((preview) => {
    const now = getSchCarry();
    if (now && now.seq === carry.seq) setSchCarry({ ...now, preview });
  });
}

/** The tool now holds `item` (`prepItemForPlacement` / `addSymbol`): it rides the cursor, drawn as the server draws its command. */
export function carryItem(ctx: Pick<PlaceContext, "api">, tool: CarryTool, item: CarriedItem, at: [number, number], queue: CarriedItem[] = [], keep = false): void {
  const st = ctx.api.getState();
  const carry: SchCarry = { tool, item, queue, keep, sheet: [...st.currentSheetPath], at, preview: null, seq: ++seq };
  setSchCarry(carry);
  fetchPreview(carry);
}

/** The carried item changed (turned, mirrored, edited, incremented): the old drawing stays until the new one arrives. */
export function changeCarried(item: CarriedItem): void {
  const now = getSchCarry();
  if (!now) return;
  const next: SchCarry = { ...now, item, seq: ++seq, editing: false };
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
  rotDeg?: number;
  mirror?: "x" | "y" | null;
  placeAllUnits?: boolean;
  /** `placeOneOnly`: a symbol the action was handed; the tool ends after it is placed. */
  oneOnly?: boolean;
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
  };
  carryItem(ctx, "sch_place_symbol", item, at, [], keep);
}

/** The second click (or the tool's hotkey): the carried item goes on the sheet at `at`, and the tool holds what comes next. */
export function placeCarried(ctx: Pick<PlaceContext, "api"> & Partial<Pick<PlaceContext, "dispatch">>, at: [number, number]): void {
  const carry = getSchCarry();
  const sch = ctx.api.getState().schematic;
  if (!carry || !sch) return;
  const plainSpin = carry.item.kind === "label" && !carry.item.spin ? inferSpin(sch.wires, at) : null;
  // `PlaceSymbol` numbers a power symbol by the project's Annotate method and start number (r3 D3-M5)
  const a = rememberedAnnotateSettings();
  void ctx.api.cmd(placeCmd(carry.item, at, { plainSpin, annotate: { algo: a.algo, start_number: startNumber(a.start) } }));
  const placed = carry.item;
  // the units of the reference already on the sheet, the one just placed among them (`unitOccupied`), and the next free reference past it (`annotate()`)
  const next = afterPlaced(
    carry,
    placed.kind === "symbol"
      ? { occupied: (u) => u === placed.unit || sch.symbols.some((x) => x.id === placed.ref && x.unit === u), nextRef: () => newSymbolRef(ctx, placed.prefix, [placed.ref]) }
      : undefined
  );
  if (next) carryItem(ctx, next.tool, next.item, at, next.queue, next.keep);
  else setSchCarry(null);
  // `if( placeOneOnly ) { m_frame->PopTool( aEvent ); break; }`
  if (placed.kind === "symbol" && placed.oneOnly) ctx.dispatch?.({ type: "SET_ACTIVE_TOOL", tool: "select" });
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

/** Properties of the carried item (`SCH_EDIT_TOOL::Properties` on the selection, which is the item): its dialog opens on it, and OK changes it. */
function reopenDialog(ctx: PlaceContext, carry: SchCarry): void {
  const at = cursorAt(ctx);
  const item = carry.item;
  if (item.kind === "power" || item.kind === "symbol") return;
  setSchCarry({ ...carry, editing: true });
  if (item.kind === "label") ctx.dispatch({ type: "SET_SCH_LABEL_PENDING", pending: { at, scope: item.scope } });
  else if (item.kind === "text") ctx.dispatch({ type: "SET_SCH_TEXT_PENDING", pending: { at } });
  else ctx.dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "directive", at: { x: at[0], y: at[1] } } });
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
