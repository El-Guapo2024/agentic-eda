// The schematic editor's edit and drawing tools that the hotkey sweeps left unwired
// (docs/parity/UI-ACTIONS.md: eeschema.InteractiveEdit / InteractiveDrawing / PointEditor), registered into
// useActionRunner's handler map. Every handler cites the KiCad function it ports (eeschema/tools at 8303b2ad);
// the logic with no React in it lives in `kicad-port/sch*.ts` with unit tests, the verbs in crates/ops/src/sch_edit.rs.
import type { Dispatch } from "react";
import type { Cmd, Schematic } from "../api/types";
import { GRID } from "../components/schematic/layout";
import { inferSpin } from "../components/schematic/labelShape";
import { allItems } from "../components/schematic/schItems";
import { measureStrokeText } from "../components/text/strokeFont";
import { alignToGrid } from "../kicad-port/gridSnap";
import { convertCmds, type ConvertSource, type ConvertTarget } from "../kicad-port/schConvertText";
import { lockCmd, type LockMode } from "../kicad-port/schLock";
import { schematicActions, type ActionMap } from "./schActionRegistry";
import { registerSchModuleSheetActions } from "./schModuleSheetActions";
import { registerSchSheetPinActions } from "./schSheetPinActions";
import { registerSchSymbolActions } from "./schSymbolActions";
import { beginBreak } from "../components/schematic/schBreakTool";
import { deleteLastPoint, finishShapeDraw } from "../components/schematic/schShapeTools";
import type { BreakMode } from "../kicad-port/schBreak";
import { nextUnitToPlace, unitCountOf } from "../kicad-port/schUnits";
import { addCorner, canAddCorner, canRemoveCorner, removeCorner } from "../kicad-port/schPolyCorners";
import { swapUnitLabels } from "../kicad-port/schSwapLabels";
import { planConvertStackedPins, planExplodeStackedPin } from "../kicad-port/stackedPins";
import { resolveLibSymbol } from "../components/schematic/libSymbol";
import { polygonOutline } from "../components/schematic/schSelectionSummary";
import type { Action, StudioApi, StudioState, ToolId } from "../state/store";
import type { SymbolEditorApi, SymAction } from "../state/symbolEditorStore";

export interface SchEditContext {
  state: StudioState;
  dispatch: Dispatch<Action>;
  api: StudioApi;
  symApi: SymbolEditorApi;
  symDispatch: Dispatch<SymAction>;
  /** `SCH_SELECTION_TOOL::RequestSelection`: the selection, or the item under the cursor. */
  requestSelection: () => string[];
  /** Adopt the hovered item as the selection when nothing is selected. */
  adoptHovered: () => string[];
  /** The cursor, grid-snapped (um), or null when the pointer is off the canvas. */
  cursorSnapped: () => [number, number] | null;
}

export function registerSchEditActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, api, requestSelection } = ctx;
  const m = schematicActions(registry, state.tab);
  const schematicOnly = (fn: () => void) => () => {
    if (state.tab === "schematic") fn();
  };
  const sch = state.schematic;

  // Lock / Unlock / Toggle Lock -- SCH_EDIT_TOOL::modifyLockSelected: RequestSelection, then `SetLocked` on every selected item (Toggle: unlock
  // when any selected item is locked, else lock). Which items change state is decided in kicad-port/schLock.ts; one verb, one undo step.
  const lock = (mode: LockMode) =>
    schematicOnly(() => {
      if (!sch) return;
      const cmd = lockCmd(mode, requestSelection(), new Set(sch.locked ?? []));
      if (cmd) void api.cmd({ op: "sch_edit", ...cmd });
    });
  m.set("eeschema.InteractiveEdit.lock", lock("lock"));
  m.set("eeschema.InteractiveEdit.unlock", lock("unlock"));
  m.set("eeschema.InteractiveEdit.toggleLock", lock("toggle"));

  // Change To Label / Global Label / Hierarchical Label / Directive Label / Text / Text Box -- SCH_EDIT_TOOL::ChangeTextType (kicad-port/schConvertText.ts):
  // every selected label, text, text box or directive label that is not already of the target type is replaced by one that is, in one undo step, and the
  // new items become the selection.
  const convert = (target: ConvertTarget) =>
    schematicOnly(() => {
      if (!sch) return;
      const sources = convertSources(sch, requestSelection());
      const { cmds, predictedIds } = convertCmds(sources, target, {
        measure: measureStrokeText,
        snap: (p) => {
          const a = alignToGrid({ x: p[0], y: p[1] }, GRID, { x: 0, y: 0 }, { ctrlOrCmd: false });
          return [a.x, a.y];
        },
        takenIds: new Set(allItems(sch).map((r) => r.id)),
      });
      if (cmds.length === 0) return;
      void api.cmdBatch(cmds).then((ok) => {
        if (ok) ctx.dispatch({ type: "SET_SELECTION", refs: predictedIds });
      });
    });
  m.set("eeschema.InteractiveEdit.toLabel", convert("label"));
  m.set("eeschema.InteractiveEdit.toGLabel", convert("global_label"));
  m.set("eeschema.InteractiveEdit.toHLabel", convert("hier_label"));
  m.set("eeschema.InteractiveEdit.toCLabel", convert("directive_label"));
  m.set("eeschema.InteractiveEdit.toText", convert("text"));
  m.set("eeschema.InteractiveEdit.toTextBox", convert("text_box"));

  // Draw Rectangles / Circles / Arcs / Bezier Curve / Text Boxes / Rule Areas and Place Directive Labels -- SCH_DRAWING_TOOLS::DrawShape / DrawRuleArea / TwoClickPlace:
  // each arms its tool, which stays armed for the next item until Esc (the click behaviour is components/schematic/schShapeTools.ts, called by SchematicView).
  const arm = (tool: ToolId) =>
    schematicOnly(() => {
      // On a nested sheet the item lands on that sheet: `api.cmd` addresses every schematic command to the sheet in view (kicad-port/schSheetCmd.ts).
      ctx.dispatch({ type: "SET_DRAW_STATE", draw: null });
      ctx.dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === tool ? "select" : tool });
    });
  m.set("eeschema.InteractiveDrawing.drawRectangle", arm("sch_rect"));
  m.set("eeschema.InteractiveDrawing.drawCircle", arm("sch_circle"));
  m.set("eeschema.InteractiveDrawing.drawArc", arm("sch_arc"));
  m.set("eeschema.InteractiveDrawing.drawBezier", arm("sch_bezier"));
  m.set("eeschema.InteractiveDrawing.drawTextBox", arm("sch_textbox"));
  m.set("eeschema.InteractiveDrawing.drawRuleArea", arm("sch_rule_area"));
  m.set("eeschema.InteractiveDrawing.placeClassLabel", arm("sch_directive"));

  // Create Corner / Remove Corner -- SCH_POINT_EDITOR::addCorner / removeCorner on the single selected polygon or rule area (kicad-port/schPolyCorners.ts). Like the menu
  // entries (`addCornerCondition` / `removeCornerCondition`) they exist only with the pointer on the outline / on one of its corners.
  const polyId = state.selection.size === 1 ? [...state.selection][0]! : null;
  const poly = sch && polyId ? polygonOutline(sch, polyId) : null;
  if (sch && polyId && poly && state.cursorUm) {
    const raw: [number, number] = [state.cursorUm.x, state.cursorUm.y];
    const tol = 10 / (state.schematicView.scale || 1); // EDIT_POINT::POINT_SIZE
    const replace = (pts: ReadonlyArray<readonly [number, number]>) => {
      const g = (sch.graphics ?? []).find((x) => x.id === polyId);
      if (!g || (g.shape.type !== "polygon" && g.shape.type !== "rule_area")) return;
      void api.cmd({ op: "sch_edit", verb: "edit_graphic", id: polyId, graphic: { ...g, shape: { ...g.shape, pts: pts.map(([x, y]) => ({ x, y })) } } });
    };
    if (canAddCorner(poly.pts, raw, tol)) {
      m.set(
        "eeschema.PointEditor.addCorner",
        schematicOnly(() => {
          const at = ctx.cursorSnapped();
          if (at) replace(addCorner(poly.pts, at));
        })
      );
    }
    if (canRemoveCorner(poly.kind, poly.pts, raw, tol)) {
      m.set(
        "eeschema.PointEditor.removeCorner",
        schematicOnly(() => {
          const next = removeCorner(poly.kind, poly.pts, raw, tol);
          if (next) replace(next);
        })
      );
    }
  }

  // Swap Unit Labels -- SCH_EDIT_TOOL::SwapUnitLabels: the units of the selected multi-unit reference trade the net labels on their pins (kicad-port/schSwapLabels.ts).
  // Offered, like the menu entry (`GetSameSymbolMultiUnitSelection`), for one reference with several units placed.
  const unitRef = sch && state.selection.size === 1 ? [...state.selection][0]! : null;
  if (sch && unitRef && sch.symbols.filter((s) => s.id === unitRef).length > 1) {
    m.set(
      "eeschema.InteractiveEdit.swapUnitLabels",
      schematicOnly(() => {
        const info = (message: string, kind: "info" | "error" = "info") => ctx.dispatch({ type: "TOAST", message, kind });
        const pinTips = (s: (typeof sch.symbols)[number]): Array<[number, number]> => resolveLibSymbol(s, sch.lib_symbols)?.pins.map((p) => [p.tip[0], p.tip[1]] as [number, number]) ?? [];
        const placed = sch.symbols.filter((s) => s.id === unitRef);
        const result = swapUnitLabels(
          {
            wires: sch.wires.map((w) => ({ id: w.id, pts: w.pts, bus: w.bus })),
            labels: sch.labels.map((l) => ({ id: l.id, at: l.at, net: l.net })),
            pinPoints: [...sch.symbols.flatMap(pinTips), ...sch.power_symbols.map((p) => [p.at[0], p.at[1]] as [number, number])],
          },
          placed.map((s) => ({ unit: s.unit, tips: pinTips(s) }))
        );
        if (!result.ok) return info(result.message, "error");
        // A label's text cannot be edited in place: each changed label is replaced by one of the new text at the same spot, one undo step.
        const cmds: Cmd[] = [];
        for (const e of result.edits) {
          const l = sch.labels.find((x) => x.id === e.id);
          if (!l) continue;
          cmds.push({ op: "delete_label", id: l.id });
          cmds.push({ op: "add_label", net: e.net, at: { x: l.at[0], y: l.at[1] }, kind: l.scope === "local" ? { scope: "local" } : { scope: l.scope, shape: l.shape ?? "passive" } });
        }
        if (cmds.length > 0) void api.cmdBatch(cmds);
      })
    );
  }

  // Place Next Symbol Unit -- SCH_DRAWING_TOOLS::PlaceNextSymbolUnit: one selected multi-unit symbol; the lowest unit of it not yet on the sheet is armed for placement under the
  // same reference, value and footprint (kicad-port/schUnits.ts); anything else says why not (the info-bar messages).
  m.set(
    "eeschema.InteractiveDrawing.placeNextSymbolUnit",
    schematicOnly(() => {
      if (!sch) return;
      const ids = requestSelection().filter((id) => sch.symbols.some((s) => s.id === id));
      const info = (message: string) => ctx.dispatch({ type: "TOAST", message, kind: "info" });
      if (ids.length !== 1) return info("Select a single symbol to place the next unit.");
      const placed = sch.symbols.filter((s) => s.id === ids[0]);
      const first = placed[0];
      if (!first?.lib_id) return;
      const next = nextUnitToPlace(unitCountOf(sch.lib_symbols[first.lib_id]), placed.map((s) => s.unit));
      if (!next.ok) return info(next.message);
      ctx.dispatch({ type: "SET_ARMED_SYMBOL", symbol: { libId: first.lib_id, referencePrefix: "", unit: next.unit, ref: first.id, value: first.value ?? undefined, footprint: first.footprint ?? undefined } });
      ctx.dispatch({ type: "SET_ACTIVE_TOOL", tool: "sch_place_symbol" });
    })
  );

  // Break and Slice -- SCH_MOVE_TOOL in BREAK / SLICE mode: the selected wires, buses and lines are cut at the cursor (one) or at their midpoints (several) and the new end
  // follows the cursor until the click that drops it (components/schematic/schBreakTool.ts).
  const startBreakTool = (mode: BreakMode) =>
    schematicOnly(() => {
      const at = ctx.cursorSnapped();
      if (!sch || !at) return;
      const brk = beginBreak(sch, requestSelection(), mode, at);
      if (!brk) return;
      ctx.dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", brk } });
      ctx.dispatch({ type: "SET_ACTIVE_TOOL", tool: "sch_break" });
    });
  m.set("eeschema.InteractiveEdit.breakWire", startBreakTool("break"));
  m.set("eeschema.InteractiveEdit.slice", startBreakTool("slice"));

  // Close Outline (End) and Delete Last Point (Backspace) -- DrawRuleArea's loop: `closeOutline` finishes the polygon as a double-click does, `deleteLastPoint`
  // (also Delete and Undo while drawing) drops the last corner and cancels the rule area when none is left.
  const draw = state.drawState?.kind === "sch_shape" ? state.drawState : null;
  if (state.activeTool === "sch_rule_area") {
    m.set(
      "eeschema.InteractiveDrawing.closeOutline",
      schematicOnly(() => {
        const at = ctx.cursorSnapped();
        if (sch && at) finishShapeDraw({ tool: state.activeTool, draw: state.drawState, sch, lineMode: state.schLineMode, dispatch: ctx.dispatch, api }, at);
      })
    );
  }
  if (draw?.poly) m.set("eeschema.InteractiveDrawing.deleteLastPoint", schematicOnly(() => deleteLastPoint(draw, ctx.dispatch)));

  registerSchSheetPinActions(registry, ctx);
  registerSchModuleSheetActions(registry, ctx);
  registerSchSymbolActions(registry, ctx);
  registerStackedPinActions(registry, ctx);
}

/**
 * Convert Stacked Pins / Explode Stacked Pin -- SYMBOL_EDITOR_EDIT_TOOL::ConvertStackedPins / ExplodeStackedPin (kicad-port/stackedPins.ts). They act on the open symbol's
 * selected pins, so they exist on the Symbol Editor tab only (and go into the raw registry, not the schematic-only wrapper); the whole change is one undo step.
 */
function registerStackedPinActions(m: ActionMap, ctx: SchEditContext): void {
  if (ctx.state.tab !== "symbol") return;
  const stack = (plan: typeof planConvertStackedPins) => () => {
    const st = ctx.symApi.getState();
    if (!st.symbol || !st.libId) return;
    const result = plan(st.libId, st.symbol.pins, [...st.selection]);
    if (!result.ok) return ctx.symDispatch({ type: "TOAST", message: result.message, kind: "error" });
    ctx.symDispatch({ type: "SET_SELECTION", refs: [] }); // "Clear selection before modifying pins, like the Delete command does"
    void ctx.symApi.cmd({ op: "batch", cmds: result.cmds });
  };
  m.set("eeschema.InteractiveEdit.convertStackedPins", stack(planConvertStackedPins));
  m.set("eeschema.InteractiveEdit.explodeStackedPin", stack(planExplodeStackedPin));
}

/** The labels, texts, text boxes and directive labels among `ids`, as the conversion's sources (a label's spin is read off its wire, as the painter does). */
function convertSources(sch: Schematic, ids: readonly string[]): ConvertSource[] {
  const out: ConvertSource[] = [];
  for (const id of ids) {
    const l = sch.labels.find((x) => x.id === id);
    if (l) {
      out.push({ kind: "label", id, net: l.net, at: l.at, scope: l.scope, shape: l.shape, spin: l.spin ?? inferSpin(sch.wires, l.at) });
      continue;
    }
    const t = sch.texts.find((x) => x.id === id);
    if (t) {
      out.push({ kind: "text", id, content: t.content, at: t.at, angleDeg: t.angle, sizeUm: t.size_um });
      continue;
    }
    const g = (sch.graphics ?? []).find((x) => x.id === id);
    if (g?.shape.type === "text_box") out.push({ kind: "text_box", id, graphic: g });
    else if (g?.shape.type === "directive") out.push({ kind: "directive", id, graphic: g });
  }
  return out;
}
