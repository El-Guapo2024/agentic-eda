// The schematic's shape tools -- Draw Rectangle / Circle / Arc / Bezier Curve / Text Box (`SCH_DRAWING_TOOLS::DrawShape`) and Draw Rule Area
// (`DrawRuleArea`): what a click does in each, the preview painted while one is in progress, and what finishing commits. A text box is DrawShape's: the
// first click is a corner, the box follows the cursor, the second click is the other corner, and only then does Text Box Properties open
// (`DIALOG_TEXT_PROPERTIES`); OK adds it, selected, and the tool stays armed. The geometry state machines are kicad-port/schShapeEdit.ts and polygonGeom.ts; the verbs are `sch_edit` `add_graphic`.
//
// Draw Arc is KiCad 25ec2335's (UI-LEFT S13): `EE_GRAPHIC_TOOL::DrawArc` drives an `ARC_DRAW_BEHAVIOR`, whose `ARC_GEOM_MANAGER` takes three clicks in its
// default mode (`ARC_DRAW_MODE::CENTER_START_END`): the centre, the start (which fixes the radius), then a point on the end radius; the direction follows the
// cursor until it is 90 degrees away (`setEnd`'s `m_directionLocked`), `/` (`arcPosture`, `ToggleClockwise`) reverses it, Backspace (`deleteLastPoint`) steps
// back a point. The same manager as the board's (kicad-port/arcGeom.ts, unchanged between the two commits for this mode); the grid snaps the points (no angle
// snap: the schematic's loop sets none). The arc is committed as the board commits one (`ApplyToShape`: the start and end in increasing angle), selected, and
// the tool stays armed (`SHAPE_DRAW_RESULT::NEXT_SHAPE`). 8303b2ad's `DrawShape` had two clicks and a 90-degree arc (`calcEdit`'s state 1: schShapeEdit.ts).
// Not ported: 25ec2335's other four arc modes (start-end-mid, start-end-centre, tangent, start-direction-end) and their toolbar palette.
import type { Dispatch } from "react";
import type { Schematic } from "../../api/types";
import type { SchGraphic, SchGraphicInput, SchGraphicShape } from "../../api/schEditTypes";
import type { Action, DrawState, StudioApi, ToolId, ViewTransform } from "../../state/store";
import { addPoint, deleteLastCorner, finalOutline, newPointClosesOutline, newPolyGeom, setCursorPosition, withMode, type LeaderMode, type PolyGeom } from "../../kicad-port/polygonGeom";
import { LINE_MODE_FREE, type LineMode } from "../../kicad-port/schLineMode";
import { graphicId } from "../../kicad-port/schIds";
import { beginEdit, calcEdit, continueEdit, toShape, type ShapeToolKind } from "../../kicad-port/schShapeEdit";
import { ARC_SET_ANGLE, ARC_SET_ORIGIN, arcClick, arcRemoveLastPoint, arcToThreePoints, type ArcGeom } from "../../kicad-port/arcGeom";
import { drawArcPreview } from "../canvas/arcPreview";
import type { LengthUnit } from "../../state/units";
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

/** Does this tool's click go through `shapeToolClick`? (Place Directive Label is a placing tool with an item in the hand: actions/schPlaceActions.ts.) */
export const isSchShapeTool = (tool: ToolId): boolean => tool in SHAPE_TOOL_KIND || tool === "sch_rule_area";

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
  if (kind === "arc") {
    arcToolClick(env, draw?.arc ?? null, at);
    return;
  }
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

/** The arc a construction stands for, as the verbs take it: `ApplyToShape` (the start and end in increasing angle) and `GetArcMid`; null while it has no sweep. */
export function arcShapeOf(g: ArcGeom): SchGraphicShape | null {
  const a = arcToThreePoints(g);
  if (!a) return null;
  return { type: "arc", start: xy(a.start), mid: xy(a.mid), end: xy(a.end) };
}

/**
 * One click of Draw Arc (`drawManagedShape`'s `IsClick( BUT_LEFT )`: `PlacePoint`, `aBehavior.AddPoint( cursorPos, true )`): the first starts the construction
 * (`ACTIONS::selectionClear`), the third completes it -- the arc is committed, selected, and a new one may start. A point the manager refuses (the start on the
 * centre, the end on the start radius) steps it back, as `MULTISTEP_GEOM_MANAGER::AddPoint` does.
 */
function arcToolClick(env: ShapeToolEnv, prev: ArcGeom | null, at: readonly [number, number]): void {
  if (!prev) env.dispatch({ type: "SET_SELECTION", refs: [] });
  const { geom, arc } = arcClick(prev, [at[0], at[1]], false);
  if (arc) {
    env.dispatch({ type: "SET_DRAW_STATE", draw: null });
    commitGraphic(env, { shape: { type: "arc", start: xy(arc.start), mid: xy(arc.mid), end: xy(arc.end) } });
    return;
  }
  env.dispatch({ type: "SET_DRAW_STATE", draw: geom ? { kind: "sch_shape", arc: geom } : null });
}

/** `ACTIONS::finishInteractive` / `closeOutline`: end whatever is being drawn as it stands. */
export function finishShapeDraw(env: ShapeToolEnv, cursor: readonly [number, number]): void {
  const draw = env.draw?.kind === "sch_shape" ? env.draw : null;
  if (!draw) return;
  if (draw.poly) finishRuleArea(env, setCursorPosition(withMode(draw.poly, leaderModeOf(env.lineMode)), [cursor[0], cursor[1]]));
  else if (draw.shape) finishShape(env, calcEdit(draw.shape, [cursor[0], cursor[1]]));
  else if (draw.arc) {
    // an arc that has its end angle goes in as it stands (a click at the cursor completes it); one short of that is dropped
    env.dispatch({ type: "SET_DRAW_STATE", draw: null });
    const shape = draw.arc.step === ARC_SET_ANGLE ? arcShapeOf(draw.arc) : null;
    if (shape) commitGraphic(env, { shape });
  }
}

/** `deleteLastPoint` (Backspace, or Undo, while a rule area is in progress): drop the last corner, and the whole item when none is left. */
export function deleteLastPoint(draw: SchShapeDraw, dispatch: Dispatch<Action>): void {
  if (draw.arc) {
    // `MULTISTEP_GEOM_MANAGER::RemoveLastPoint`: back a step; back at the centre, nothing is left of the arc
    const g = arcRemoveLastPoint(draw.arc);
    dispatch({ type: "SET_DRAW_STATE", draw: g.step === ARC_SET_ORIGIN ? null : { kind: "sch_shape", arc: g } });
    return;
  }
  if (!draw.poly) return;
  const { geom } = deleteLastCorner(draw.poly);
  dispatch({ type: "SET_DRAW_STATE", draw: geom.locked.length === 0 ? null : { kind: "sch_shape", poly: geom } });
}

/** The graphic a shape in progress would be, at the cursor -- what the preview paints (`m_view->AddToPreview( item->Clone() )`). */
export function previewShape(draw: SchShapeDraw, cursor: readonly [number, number]): SchGraphicShape | null {
  if (draw.arc) return arcShapeOf(draw.arc);
  if (!draw.shape) return null;
  const e = calcEdit(draw.shape, [cursor[0], cursor[1]]);
  if (e.kind === "text_box") return e.start[0] === e.end[0] && e.start[1] === e.end[1] ? null : { type: "rectangle", start: xy(e.start), end: xy(e.end) };
  return toShape(e);
}

/** Paint the shape or rule area being drawn: the rubber-banded item for a shape, the locked corners, leader and closing loop for a rule area. */
export function paintShapePreview(ctx: CanvasRenderingContext2D, view: ViewTransform, draw: SchShapeDraw, cursor: readonly [number, number], lineMode: LineMode, units: LengthUnit = "mm"): void {
  if (draw.arc) {
    // `ARC_ASSISTANT` over the arc as it would be drawn: the radius lines, the centre, the readout
    const shape = previewShape(draw, cursor);
    if (shape) paintGraphics(ctx, view, [{ id: "__preview__", shape }], new Set());
    const scale = view.scale || 1;
    drawArcPreview(ctx, draw.arc, { color: layerColor("LAYER_NOTES"), hair: (px) => px / scale, units });
    return;
  }
  if (draw.poly) {
    const g = setCursorPosition(withMode(draw.poly, leaderModeOf(lineMode)), [cursor[0], cursor[1]]);
    const color = layerColor("LAYER_RULE_AREAS");
    ctx.save();
    ctx.lineWidth = Math.max(150_000, 1.5 / view.scale);
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
