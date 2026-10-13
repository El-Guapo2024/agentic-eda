// The 3D Models tab of the Footprint Properties dialogs: the list of a footprint's `(model ...)` entries and the fields that edit one.
//
// Ported from `pcbnew/dialogs/panel_fp_properties_3d_model.cpp` (the grid: a status cell, the file name, Show; Add, Browse, Remove) and
// `3d-viewer/dialogs/panel_preview_3d_model.cpp` (the fields beside the preview: Scale, Rotation, Offset, Opacity, with the spin buttons' steps, the wheel's, and the clamps
// of `panel_preview_3d_model.h`). The server's half is crates/model/src/fp_models.rs (the checks), crates/ops/src/fp_models.rs (the verbs) and
// crates/cli/src/model3d_tab_api.rs (the status of a name, the chooser's folders, the footprint to preview).
//
// ------------------------------------------------------------------------------------------------------------- the numbers
//
// A list here is the file's: millimetres and degrees, the rotation as the file stores it. The dialog shows the rotation the other way round ("Rotation is stored in the file
// as positive-is-CW, but we use positive-is-CCW in the GUI": `xrot->ChangeValue( formatRotationValue( -modelInfo.m_Rotation.x ) )`), so `showRotation` and `readRotation`
// carry the sign. The offset is shown in the units of the editor (`formatOffsetValue`: millimetres, inches or mils), stored in millimetres.
import type { Cmd, Model3dCmd, Part } from "../api/types";
import type { LengthUnit } from "../state/units";

export type Vec3 = [number, number, number];

/** One row of the tab, whole: what `Model3dCmd` says with its defaults filled in. */
export interface ModelRow {
  path: string;
  offset: Vec3;
  scale: Vec3;
  rotate: Vec3;
  opacity: number;
  show: boolean;
}

/** `panel_preview_3d_model.h`: the largest scale and offset the fields' spin buttons and wheel allow, the largest rotation `rotationFromString` keeps, and their steps. */
export const MAX_SCALE = 10000;
export const MAX_ROTATION = 180;
export const MAX_OFFSET = 1000;
export const SCALE_INCREMENT = 0.1;
export const SCALE_INCREMENT_FINE = 0.02;
export const ROTATION_INCREMENT = 90;
export const ROTATION_INCREMENT_FINE = 1;
export const OFFSET_INCREMENT_MM = 0.5;
export const OFFSET_INCREMENT_MM_FINE = 0.1;
export const OFFSET_INCREMENT_MIL = 25;
export const OFFSET_INCREMENT_MIL_FINE = 5;

/** A model placed as it comes (`FP_3DMODEL`'s constructor): no offset, scale 1, no rotation, opaque, shown. */
export function plainRow(path = ""): ModelRow {
  return { path, offset: [0, 0, 0], scale: [1, 1, 1], rotate: [0, 0, 0], opacity: 1, show: true };
}

/** The rows of a part's models as `GET /api/state` sends them (`Part.models`; the file name is `name` there). */
export function rowsOf(models: Part["models"] | undefined): ModelRow[] {
  return (models ?? []).map((m) => ({ path: m.name, offset: [...m.offset], scale: [...m.scale], rotate: [...m.rotate], opacity: m.opacity, show: m.show }));
}

/** A model as a command or the Footprint Editor's JSON has it -- `GET /api/footprint` leaves out what is KiCad's default -- as a whole row. */
export function rowOfCmd(m: Model3dCmd): ModelRow {
  return { path: m.path, offset: [...(m.offset ?? [0, 0, 0])], scale: [...(m.scale ?? [1, 1, 1])], rotate: [...(m.rotate ?? [0, 0, 0])], opacity: m.opacity ?? 1, show: m.show ?? true };
}

/** The rows of a library footprint: its list, else the one path of an older entry placed as it comes. */
export function rowsOfLibrary(fp: { models3d?: Model3dCmd[]; model?: string | null }): ModelRow[] {
  if (fp.models3d && fp.models3d.length > 0) return fp.models3d.map(rowOfCmd);
  return fp.model ? [plainRow(fp.model)] : [];
}

/** What the Footprint Editor's dialog sends for the models: the whole list without the rows that have no file name, and `model` as the first path (`null` for none). */
export function libraryModels(rows: readonly ModelRow[]): { model: string | null; models3d: Model3dCmd[] } {
  const kept = rows.filter((r) => r.path.trim() !== "").map((r) => ({ ...r, path: cleanPath(r.path) }));
  return { model: kept[0]?.path ?? null, models3d: kept.map(modelCmd) };
}

/** The row as a command takes it. */
export function modelCmd(row: ModelRow): Model3dCmd {
  return { path: row.path, offset: [...row.offset], scale: [...row.scale], rotate: [...row.rotate], opacity: row.opacity, show: row.show };
}

export function sameRows(a: readonly ModelRow[], b: readonly ModelRow[]): boolean {
  return a.length === b.length && a.every((r, i) => JSON.stringify(r) === JSON.stringify(b[i]));
}

/**
 * What the dialog's OK sends for the models: nothing when the list is the one the footprint has, else `set_board_models` with the whole list -- a row with no file name is left
 * out (KiCad keeps it in the grid, and does not write a model with no name: `if( !bs3D->m_Filename.IsEmpty() )`).
 */
export function boardModelsCmds(part: Part, rows: readonly ModelRow[]): Cmd[] {
  const kept = rows.filter((r) => r.path.trim() !== "").map((r) => ({ ...r, path: cleanPath(r.path) }));
  return sameRows(kept, rowsOf(part.models)) ? [] : [{ op: "set_board_models", part: part.ref, models: kept.map(modelCmd) }];
}

/** `PANEL_FP_PROPERTIES_3D_MODEL::cleanupFilename`: the line breaks and tabs a paste brings are taken out. */
export function cleanPath(name: string): string {
  return name.replace(/[\n\r\t]/g, "");
}

// ------------------------------------------------------------------------------------------------------ the fields

/**
 * `evaluateTextCtrl` (`NUMERIC_EVALUATOR`): the text of a field with an arithmetic expression worked out -- `+ - * /`, brackets, a unary minus; the text as it is when it is not an
 * expression (or holds a character that is not ASCII, as a degree sign: "NUMERIC_EVALUATOR doesn't handle UTF-8 multi-byte characters properly").
 */
export function evaluateField(text: string): string {
  if (/[^\x00-\x7f]/.test(text)) return text;
  const value = evaluateExpression(text);
  return value === null ? text : String(value);
}

/** An arithmetic expression, or null when the text is not one. */
export function evaluateExpression(text: string): number | null {
  const s = text.trim();
  if (s === "") return null;
  let i = 0;
  const peek = () => s[i];
  const skip = () => {
    while (i < s.length && s[i] === " ") i++;
  };
  const number = (): number | null => {
    skip();
    const m = /^(\d+\.?\d*|\.\d+)(e[+-]?\d+)?/i.exec(s.slice(i));
    if (!m) return null;
    i += m[0].length;
    return Number(m[0]);
  };
  const factor = (): number | null => {
    skip();
    if (peek() === "-" || peek() === "+") {
      const sign = peek() === "-" ? -1 : 1;
      i++;
      const f = factor();
      return f === null ? null : sign * f;
    }
    if (peek() === "(") {
      i++;
      const v = sum();
      skip();
      if (v === null || peek() !== ")") return null;
      i++;
      return v;
    }
    return number();
  };
  const product = (): number | null => {
    let v = factor();
    for (;;) {
      skip();
      const op = peek();
      if (v === null || (op !== "*" && op !== "/")) return v;
      i++;
      const r = factor();
      if (r === null) return null;
      v = op === "*" ? v * r : v / r;
    }
  };
  const sum = (): number | null => {
    let v = product();
    for (;;) {
      skip();
      const op = peek();
      if (v === null || (op !== "+" && op !== "-")) return v;
      i++;
      const r = product();
      if (r === null) return null;
      v = op === "+" ? v + r : v - r;
    }
  };
  const value = sum();
  skip();
  return value !== null && i === s.length && Number.isFinite(value) ? value : null;
}

/** Whether the text of a field is a number, or an expression, with a unit or the degree sign after it: a field that is half typed in ("-", "1+") is not committed yet. */
export function isNumberText(text: string): boolean {
  const m = /^(.*?)\s*(mm|mils?|thou|in|"|°|deg)?\s*$/i.exec(evaluateField(text).trim());
  return evaluateExpression(m?.[1] ?? "") !== null;
}

/** A number typed in a field (worked out if it is an expression), `fallback` when it is none. */
export function readNumber(text: string, fallback: number): number {
  const v = evaluateExpression(evaluateField(text).replace(/°|deg$/i, ""));
  return v === null ? fallback : v;
}

/** `formatScaleValue`: `%.4f`. */
export function formatScale(v: number): string {
  return v.toFixed(4);
}

/** `formatRotationValue`: `%.2f` and the degree sign, and no negative zero. */
export function formatRotation(v: number): string {
  const r = Number(v.toFixed(2));
  return `${(r === 0 ? 0 : r).toFixed(2)}°`;
}

/** `rotationFromString`: the typed angle, brought into -180..180 by KiCad's own rule (`n = round( rotation / 180 )`, `rotation -= 180 n`, which is what it does with 200: 20). */
export function rotationFromString(text: string): number {
  let rotation = readNumber(text, 0);
  if (rotation > MAX_ROTATION) {
    const n = kiRound(rotation / MAX_ROTATION);
    rotation -= MAX_ROTATION * n;
  } else if (rotation < -MAX_ROTATION) {
    const n = kiRound(-rotation / MAX_ROTATION);
    rotation += MAX_ROTATION * n;
  }
  return rotation;
}

/** `KiROUND`: halves away from zero. */
function kiRound(v: number): number {
  return v < 0 ? -Math.round(-v) : Math.round(v);
}

/** What the rotation field shows for a model's stored (file) rotation: the sign turned, counter-clockwise. */
export function showRotation(storedDeg: number): string {
  return formatRotation(-storedDeg);
}

/** The stored (file) rotation for what is typed in the field. */
export function readRotation(text: string): number {
  const v = -rotationFromString(text);
  return v === 0 ? 0 : v;
}

const MM_PER_UNIT: Record<LengthUnit, number> = { mm: 1, in: 25.4, mil: 0.0254 };
const UNIT_TEXT: Record<LengthUnit, string> = { mm: " mm", in: " in", mil: " mils" };

/** `formatOffsetValue`: the offset (stored in millimetres) in the editor's units, six decimals and the unit. */
export function formatOffset(mm: number, unit: LengthUnit): string {
  const v = mm / MM_PER_UNIT[unit];
  return `${(Object.is(v, -0) ? 0 : v).toFixed(6)}${UNIT_TEXT[unit]}`;
}

/** The offset in millimetres for what is typed: a number in the editor's units, or with its unit after it (`mm`, `in`, `"`, `mil`, `mils`, `thou`). */
export function readOffset(text: string, unit: LengthUnit): number {
  const m = /^(.*?)\s*(mm|mils?|thou|in|")?\s*$/i.exec(evaluateField(text).trim());
  const typed = (m?.[2] ?? "").toLowerCase();
  const own = typed === "" ? unit : typed === "mm" ? "mm" : typed === "in" || typed === '"' ? "in" : "mil";
  const v = evaluateExpression(m?.[1] ?? "");
  // six decimals are what the field shows (`%.6f`): a conversion from inches does not leave float noise in the file
  return v === null ? 0 : Math.round(v * MM_PER_UNIT[own] * 1e6) / 1e6;
}

/** `doIncrementScale` and the wheel (`onMouseWheelScale`): the scale one step on, kept between 1/MAX_SCALE and MAX_SCALE. */
export function stepScale(current: number, sign: 1 | -1, fine: boolean): number {
  const v = current + (fine ? SCALE_INCREMENT_FINE : SCALE_INCREMENT) * sign;
  return Math.min(Math.max(1 / MAX_SCALE, v), MAX_SCALE);
}

/** `doIncrementRotation` and the wheel: the angle shown one step on (90 degrees, 1 with Shift), in the field's sign. */
export function stepRotation(shown: number, sign: 1 | -1, fine: boolean): number {
  return shown + (fine ? ROTATION_INCREMENT_FINE : ROTATION_INCREMENT) * sign;
}

/** `doIncrementOffset` and the wheel: the offset (millimetres) one step on: 0.5 mm (0.1 with Shift), or 25 mils (5) in inches and mils; kept within MAX_OFFSET. */
export function stepOffset(mm: number, sign: 1 | -1, fine: boolean, unit: LengthUnit): number {
  const step = unit === "mm" ? (fine ? OFFSET_INCREMENT_MM_FINE : OFFSET_INCREMENT_MM) : (25.4 * (fine ? OFFSET_INCREMENT_MIL_FINE : OFFSET_INCREMENT_MIL)) / 1000;
  return Math.min(Math.max(-MAX_OFFSET, mm + step * sign), MAX_OFFSET);
}

/** The opacity slider (0 to 100) and the stored opacity (0 to 1). */
export const sliderOf = (opacity: number): number => Math.round(opacity * 100);
export const opacityOf = (slider: number): number => slider / 100;

// ----------------------------------------------------------------------------------------------------------- statuses

/** What the status cell of a row says (`MODEL_VALIDATE_ERRORS`, and the studio's own `forbidden`). */
export type ModelStatus = "ok" | "empty" | "illegal" | "missing" | "forbidden";

export interface ModelCheck {
  name: string;
  status: ModelStatus;
  message: string;
}

/** `GRID_CELL_STATUS_ICON_RENDERER`: a warning for no name, an error for the others, nothing for a model that is there. */
export function statusIcon(status: ModelStatus | undefined): "" | "warning" | "error" {
  return status === undefined || status === "ok" ? "" : status === "empty" ? "warning" : "error";
}

/** The order of the rows after the one at `from` is moved to `to` (the list's own rule: [`move_model`] in crates/model/src/fp_models.rs). */
export function movedRows<T>(rows: readonly T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= rows.length || to >= rows.length) return [...rows];
  const out = [...rows];
  const [m] = out.splice(from, 1);
  out.splice(to, 0, m!);
  return out;
}
