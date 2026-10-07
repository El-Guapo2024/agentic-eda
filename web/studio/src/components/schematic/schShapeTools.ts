// The schematic's shape tools -- Draw Rectangle / Circle / Arc / Bezier Curve / Text Box (`SCH_DRAWING_TOOLS::DrawShape`), Draw Rule Area
// (`DrawRuleArea`) and Place Directive Label (`TwoClickPlace`): what a click does in each, the preview painted while one is in progress, and what
// finishing commits. The geometry state machines are kicad-port/schShapeEdit.ts and polygonGeom.ts; the verbs are `sch_edit` `add_graphic`.
import type { Dispatch } from "react";
import type { Schematic } from "../../api/types";
import type { SchGraphic, SchGraphicInput, SchGraphicShape } from "../../api/schEditTypes";
import type { Action, DrawState, StudioApi, ToolId, ViewTransform } from "../../state/store";
import { addPoint, deleteLastCorner, finalOutline, newPointClosesOutline, newPolyGeom, setCursorPosition, withMode, type LeaderMode, type PolyGeom } from "../../kicad-port/polygonGeom";
import { LINE_MODE_FREE, type LineMode } from "../../kicad-port/schLineMode";
import { graphicId } from "../../kicad-port/schIds";
import { beginEdit, calcEdit, continueEdit, toShape, type ShapeToolKind } from "../../kicad-port/schShapeEdit";
import { layerColor } from "../canvas/layers";
import { paintGraphics } from "./schGraphicsPainter";
import { allItems } from "./schItems";

export type SchShapeDraw = Extract<DrawState, { kind: "sch_shape" }>;

/** The tools that draw one of `SCH_SHAPE`'s kinds by `EDA_SHAPE::beginEdit`/`continueEdit`. */
export const SHAPE_TOOL_KIND: Partial<Record<ToolId, ShapeToolKind>> = {
  sch_rect: "rectangle",
  sch_circle: "circle",
  sch_arc: "arc",
  sch_bezier: "bezier",
  sch_textbox: "text_box",
};

/** Does this tool's click go through `shapeToolClick`? */
export const isSchShapeTool = (tool: ToolId): boolean => tool in SHAPE_TOOL_KIND || tool === "sch_rule_area" || tool === "sch_directive";

/** The rule area's leader mode: `line_mode == LINE_MODE_FREE ? DIRECT : DEG45`, set on every event of `DrawRuleArea`'s loop. */
export const leaderModeOf = (lineMode: LineMode): LeaderMode => (lineMode === LINE_MODE_FREE ? "direct" : "deg45");

const xy = (p: readonly [number, number]) => ({ x: p[0], y: p[1] });

export interface ShapeToolEnv {
  tool: ToolId;
  draw: DrawState | null;
  sch: Schematic;
  lineMode: LineMode;
  dispatch: Dispatch<Action>;
  api: StudioApi;
}

/** Add a graphic and select it when it lands (`m_selectionTool->AddItemToSel( item )` after the commit). */
export function commitGraphic(env: Pick<ShapeToolEnv, "sch" | "dispatch" | "api">, graphic: SchGraphicInput): void {
  const id = graphicId(graphic.shape, new Set(allItems(env.sch).map((r) => r.id)));
  void env.api.cmd({ op: "sch_edit", verb: "add_graphic", graphic }).then((ok) => {
    if (ok) env.dispatch({ type: "SET_SELECTION", refs: [id] });
  });
}

/** What finishing a shape commits: a text box asks for its text first, any other shape goes straight in. */
function finishShape(env: ShapeToolEnv, edit: Parameters<typeof toShape>[0]): void {
  env.dispatch({ type: "SET_DRAW_STATE", draw: null });
  if (edit.kind === "text_box") {
    const [sx, sy] = edit.start;
    const [ex, ey] = edit.end;
    if (sx === ex && sy === ey) return;
    env.dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "text_box", start: xy(edit.start), end: xy(edit.end) } });
    return;
  }
  const shape = toShape(edit);
  if (shape) commitGraphic(env, { shape });
}

/** `RULE_AREA_CREATE_HELPER::OnComplete` + `commitRuleArea`: the closed outline, or nothing when fewer than three corners were clicked. */
export function finishRuleArea(env: Pick<ShapeToolEnv, "sch" | "dispatch" | "api">, poly: PolyGeom): void {
  env.dispatch({ type: "SET_DRAW_STATE", draw: null });
  const outline = finalOutline(poly);
  if (!outline) return;
  commitGraphic(env, { shape: { type: "rule_area", pts: outline.map(xy) }, line_style: "dash" });
}

/**
 * One click of a shape tool, `at` already snapped. A first click begins the item (`BeginEdit`); a further one continues it (`ContinueEdit`) and
 * finishes it when the shape needs no more points. `double` is a double-click (`IsDblClick`), which finishes whatever is in progress as it stands.
 */
export function shapeToolClick(env: ShapeToolEnv, at: readonly [number, number], double = false): void {
  const { tool, dispatch } = env;
  if (tool === "sch_directive") {
    dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "directive", at: xy(at) } });
    return;
  }
  const draw = env.draw?.kind === "sch_shape" ? env.draw : null;
  if (tool === "sch_rule_area") {
    const mode = leaderModeOf(env.lineMode);
    const poly = withMode(draw?.poly ?? newPolyGeom(mode), mode);
    if (double || newPointClosesOutline(poly, [at[0], at[1]])) {
      finishRuleArea(env, setCursorPosition(poly, [at[0], at[1]]));
      return;
    }
    if (!draw?.poly) dispatch({ type: "SET_SELECTION", refs: [] });
    // The tool lays the leader out for the cursor on every motion; the click then locks it in.
    dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", poly: addPoint(setCursorPosition(poly, [at[0], at[1]]), [at[0], at[1]]) } });
    return;
  }
  const kind = SHAPE_TOOL_KIND[tool];
  if (!kind) return;
  if (!draw?.shape) {
    dispatch({ type: "SET_SELECTION", refs: [] });
    dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", shape: beginEdit(kind, [at[0], at[1]]) } });
    return;
  }
  const moved = calcEdit(draw.shape, [at[0], at[1]]);
  if (double) {
    finishShape(env, moved);
    return;
  }
  const next = continueEdit(moved);
  if (next.more) dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", shape: next.edit } });
  else finishShape(env, next.edit);
}

/** `ACTIONS::finishInteractive` / `closeOutline`: end whatever is being drawn as it stands. */
export function finishShapeDraw(env: ShapeToolEnv, cursor: readonly [number, number]): void {
  const draw = env.draw?.kind === "sch_shape" ? env.draw : null;
  if (!draw) return;
  if (draw.poly) finishRuleArea(env, setCursorPosition(withMode(draw.poly, leaderModeOf(env.lineMode)), [cursor[0], cursor[1]]));
  else if (draw.shape) finishShape(env, calcEdit(draw.shape, [cursor[0], cursor[1]]));
}

/** `deleteLastPoint` (Backspace, or Undo, while a rule area is in progress): drop the last corner, and the whole item when none is left. */
export function deleteLastPoint(draw: SchShapeDraw, dispatch: Dispatch<Action>): void {
  if (!draw.poly) return;
  const { geom } = deleteLastCorner(draw.poly);
  dispatch({ type: "SET_DRAW_STATE", draw: geom.locked.length === 0 ? null : { kind: "sch_shape", poly: geom } });
}

/** The graphic a shape in progress would be, at the cursor -- what the preview paints (`m_view->AddToPreview( item->Clone() )`). */
export function previewShape(draw: SchShapeDraw, cursor: readonly [number, number]): SchGraphicShape | null {
  if (!draw.shape) return null;
  const e = calcEdit(draw.shape, [cursor[0], cursor[1]]);
  if (e.kind === "text_box") return e.start[0] === e.end[0] && e.start[1] === e.end[1] ? null : { type: "rectangle", start: xy(e.start), end: xy(e.end) };
  return toShape(e);
}

/** Paint the shape or rule area being drawn: the rubber-banded item for a shape, the locked corners, leader and closing loop for a rule area. */
export function paintShapePreview(ctx: CanvasRenderingContext2D, view: ViewTransform, draw: SchShapeDraw, cursor: readonly [number, number], lineMode: LineMode): void {
  if (draw.poly) {
    const g = setCursorPosition(withMode(draw.poly, leaderModeOf(lineMode)), [cursor[0], cursor[1]]);
    const color = layerColor("LAYER_RULE_AREAS");
    ctx.save();
    ctx.lineWidth = Math.max(150, 1.5 / view.scale);
    ctx.strokeStyle = color;
    const outline = [...g.locked, ...g.leader.slice(1), ...g.loop.slice(1, -1)];
    ctx.beginPath();
    outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    ctx.globalAlpha = 0.2;
    ctx.fillStyle = color;
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.beginPath();
    g.locked.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
    ctx.setLineDash([4 / view.scale, 3 / view.scale]);
    ctx.beginPath();
    [...g.leader, ...g.loop].forEach(([x, y], i) => (i === 0 || i === g.leader.length ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
    ctx.restore();
    return;
  }
  const shape = previewShape(draw, cursor);
  if (!shape) return;
  const preview: SchGraphic = { id: "__preview__", shape };
  paintGraphics(ctx, view, [preview], new Set());
}
