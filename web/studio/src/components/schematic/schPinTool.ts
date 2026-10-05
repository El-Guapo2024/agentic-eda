// Place Pins from Sheet on the sheet (`SCH_DRAWING_TOOLS::TwoClickPlace` with `placeSheetPin`): the sheet's hierarchical labels, read from its own file,
// become pins one after another, each dropped on the border nearest the click; and the label reading the other sheet-pin actions share.
import type { Dispatch } from "react";
import { fetchSchematic } from "../../api/client";
import type { Schematic, Sheet } from "../../api/types";
import type { Action, StudioApi, ViewTransform } from "../../state/store";
import { constrainOnEdge, nextLabelToPlace, unplacedLabels, type HierLabel, type P, type PinExtent } from "../../kicad-port/schSheetPins";
import { layerColor } from "../canvas/layers";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";

/** The pin being placed: which sheet (null until a click over one picks it) and the labels still without a pin. */
export interface PinPlacement {
  sheetId: string | null;
  queue: HierLabel[];
}

/** `m_DefaultTextSize`, um. */
const PIN_TEXT_SIZE_UM = 1270;

/** The hierarchical labels of the sheet's own file, read from the sheet's page (the studio serves each sheet's content under `<path>/<sheet id>`). */
export async function loadHierLabels(currentPath: readonly string[], sheetId: string): Promise<HierLabel[]> {
  const child = await fetchSchematic([...currentPath, sheetId]);
  return child.labels.filter((l) => l.scope === "hierarchical").map((l) => ({ name: l.net, shape: l.shape ?? null }));
}

/** A pin's bounding box, estimated from `SCH_HIERLABEL::GetBodyBoundingBox`: text width plus a flag as tall as the text, its pen and its offset (1.5 text heights). */
export const pinExtentOf = (name: string): PinExtent => ({ width: measureStrokeText(name, PIN_TEXT_SIZE_UM) + 1.5 * PIN_TEXT_SIZE_UM, height: 1.5 * PIN_TEXT_SIZE_UM });

/** The sheet a click lands on (`SelectPoint( cursorPos, { SCH_SHEET_T } )`): its border, or anywhere inside it. */
export function sheetAt(sch: Schematic, p: P, tol: number): Sheet | undefined {
  return sch.sheets.find((s) => p[0] >= s.at[0] - tol && p[0] <= s.at[0] + s.size[0] + tol && p[1] >= s.at[1] - tol && p[1] <= s.at[1] + s.size[1] + tol);
}

/** What the next click would drop: the next label's pin on the border nearest `cursor`. */
export function nextPin(sch: Schematic, placement: PinPlacement, cursor: P): { sheet: Sheet; label: HierLabel; at: [number, number] } | null {
  const sheet = sch.sheets.find((s) => s.id === placement.sheetId);
  if (!sheet) return null;
  const label = nextLabelToPlace(sheet.pins, placement.queue);
  if (!label) return null;
  return { sheet, label, at: constrainOnEdge({ at: sheet.at, size: sheet.size }, cursor).at };
}

const NO_NEW_LABELS = "No new hierarchical labels found.";

/** One click of the tool: with no sheet yet, pick the one under the click and read its labels; otherwise drop the next pin. */
export async function placePinClick(env: { sch: Schematic; placement: PinPlacement; path: readonly string[]; dispatch: Dispatch<Action>; api: StudioApi }, at: P, tol: number): Promise<void> {
  const { sch, placement, dispatch, api } = env;
  const end = (message?: string) => {
    dispatch({ type: "SET_DRAW_STATE", draw: null });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
    if (message) dispatch({ type: "TOAST", message, kind: "info" });
  };
  if (!placement.sheetId) {
    const sheet = sheetAt(sch, at, tol);
    if (!sheet) {
      dispatch({ type: "TOAST", message: "Click over a sheet.", kind: "info" }); // `m_statusPopup`: "Click over a sheet."
      return;
    }
    const queue = unplacedLabels(sheet.pins, await loadHierLabels(env.path, sheet.id));
    if (queue.length === 0) return end(NO_NEW_LABELS);
    dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", pin: { sheetId: sheet.id, queue } } });
    return;
  }
  const next = nextPin(sch, placement, at);
  if (!next) return end(NO_NEW_LABELS);
  // The label this pin covers drops out of the queue straight away, so a second click before the sheet has been read again drops the next one; when none is
  // left the tool ends ("No new hierarchical labels found.").
  const remaining = placement.queue.filter((l) => l.name !== next.label.name);
  if (remaining.length === 0) end(NO_NEW_LABELS);
  else dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", pin: { sheetId: placement.sheetId, queue: remaining } } });
  const ok = await api.cmd({ op: "sch_edit", verb: "add_sheet_pin", sheet: next.sheet.id, name: next.label.name, shape: next.label.shape ?? "passive", at: { x: next.at[0], y: next.at[1] } });
  // A refused pin (the backend said no) is offered again.
  if (!ok) dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", pin: placement } });
}

/** The pin that would be dropped, drawn where the click would put it (`updatePreview`): a stub across the border with the pin's name beside it. */
export function paintPinPreview(ctx: CanvasRenderingContext2D, view: ViewTransform, sch: Schematic, placement: PinPlacement, cursor: P): void {
  const next = nextPin(sch, placement, cursor);
  if (!next) return;
  const [x, y] = next.at;
  ctx.save();
  ctx.strokeStyle = layerColor("LAYER_SHEETLABEL");
  ctx.lineWidth = Math.max(150, 1.5 / view.scale);
  ctx.beginPath();
  ctx.arc(x, y, 500, 0, Math.PI * 2);
  ctx.stroke();
  drawStrokeText(ctx, next.label.name, x + 800, y, { sizeUm: PIN_TEXT_SIZE_UM * 0.8, justify: "left", color: layerColor("LAYER_SHEETLABEL") });
  ctx.restore();
}
