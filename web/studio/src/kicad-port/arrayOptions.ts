// Create Array's options and what the dialog does with them: the defaults KiCad's dialog starts from, the text entries and how they are read
// (`DIALOG_CREATE_ARRAY::TransferDataFromWindow`), the geometry (`ARRAY_GRID_OPTIONS` / `ARRAY_CIRCULAR_OPTIONS`) and the one command (`create_array`)
// they make, and where the circular array's centre starts (`ARRAY_TOOL::CreateArray`'s `origin`: a lone item's position, else the centre of the selection).
//
// Ported from pcbnew/dialogs/dialog_create_array.cpp (`CREATE_ARRAY_DIALOG_ENTRIES`, `TransferDataFromWindow`, `validateLongEntry`,
// `calculateCircularArrayProperties`) and pcbnew/tools/array_tool.cpp at KiCad 8303b2ad; the backend half is crates/ops/src/array.rs.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { ArrayGeometry, Cmd, Um } from "../api/types";
import { umTo, type LengthUnit } from "../state/units";
import { parseValue } from "./propertyGrid";

export type ArrayTab = "grid" | "circular";

/** What the dialog holds. Lengths are um, angles degrees. */
export interface ArrayOptions {
  tab: ArrayTab;
  nx: number;
  ny: number;
  dx: Um;
  dy: Um;
  offsetX: Um;
  offsetY: Um;
  stagger: number;
  /** `m_staggerRows`; false is "Columns". */
  staggerRows: boolean;
  /** "Centre on source items" (`m_rbCentreOnSource`); false is "Source items remain in place". */
  centred: boolean;
  /** `m_radioBoxGridNumberingAxis`: false is "Horizontal, then vertical". The board editor never shows it (it sits in the pad numbering panel, which is the footprint editor's). */
  verticalFirst: boolean;
  /** `m_checkBoxGridReverseNumbering`: every other row runs the other way. Footprint editor only, like the axis above. */
  reverseAlternate: boolean;
  centerX: Um;
  centerY: Um;
  count: number;
  angleDeg: number;
  /** `m_checkBoxFullCircle`: the angle is 360 / count. */
  fullCircle: boolean;
  offsetAngleDeg: number;
  clockwise: boolean;
  rotateItems: boolean;
  /** `m_radioBtnArrangeSelection`; false is "Duplicate selection". */
  arrange: boolean;
  /** `m_radioBtnUniqueRefs` ("Assign unique reference designators"); false is "Keep existing reference designators". */
  reannotate: boolean;
}

/**
 * `CREATE_ARRAY_DIALOG_ENTRIES`: 5 x 5 at 2.54 mm, a quarter turn between four points, clockwise, unique references. Both position radios are `true` there and the
 * persister sets them in turn, so the one that ends up selected on the first open is the second, "Centre on source items".
 */
export const DEFAULT_ARRAY_OPTIONS: ArrayOptions = {
  tab: "grid",
  nx: 5,
  ny: 5,
  dx: 2540,
  dy: 2540,
  offsetX: 0,
  offsetY: 0,
  stagger: 1,
  staggerRows: true,
  centred: true,
  verticalFirst: false,
  reverseAlternate: false,
  centerX: 0,
  centerY: 0,
  count: 4,
  angleDeg: 90,
  fullCircle: false,
  offsetAngleDeg: 0,
  clockwise: true,
  rotateItems: false,
  arrange: false,
  reannotate: true,
};

/** The angle between two points of the circle: `360 / count` when "Full circle" is on (`calculateCircularArrayProperties`), else the typed one. */
export function effectiveAngleDeg(o: Pick<ArrayOptions, "fullCircle" | "count" | "angleDeg">): number {
  return o.fullCircle && o.count > 0 ? 360 / o.count : o.angleDeg;
}

/**
 * `TransferDataFromWindow`'s checks of the numbers for the current tab, as its message (`wxJoin( errors, '\n' )`: every complaint, one to a line), or null when the
 * options can be used. The one check that is not KiCad's: a grid of less than one row or column, or a circle of no point, makes nothing (the dialog's entries
 * are plain text, so that is the only way to ask for it).
 */
export function checkArrayOptions(o: ArrayOptions): string | null {
  const errors: string[] = [];
  const whole = (value: number, what: string): boolean => {
    if (Number.isInteger(value)) return true;
    errors.push(`Bad numeric value for ${what}: ${value}`);
    return false;
  };
  if (o.tab === "grid") {
    const counts = [whole(o.nx, "horizontal count"), whole(o.ny, "vertical count")];
    if (counts[0] && counts[1]) {
      if (o.nx < 1 || o.ny < 1) errors.push("A grid array needs at least one row and one column.");
      else {
        if (o.nx > 1 && o.dx === 0) errors.push(`horizontal delta of zero with ${o.nx} objects`);
        if (o.ny > 1 && o.dy === 0) errors.push(`vertical delta of zero with ${o.ny} objects`);
      }
    }
    whole(o.stagger, "stagger");
  } else if (whole(o.count, "point count")) {
    if (o.count < 1) errors.push("A circular array needs at least one point.");
    else if (o.count > 1 && Math.round(effectiveAngleDeg(o) * 1000) === 0) errors.push(`angular delta of zero with ${o.count} objects`);
  }
  return errors.length > 0 ? errors.join("\n") : null;
}

// ------------------------------------------------------------------------------------------------------------------- the dialog's entries

/** The text entries as typed: the dialog's `wxTextCtrl`s and `UNIT_BINDER`s, lengths in the display units. */
export interface ArrayEntries {
  nx: string;
  ny: string;
  dx: string;
  dy: string;
  offsetX: string;
  offsetY: string;
  stagger: string;
  centerX: string;
  centerY: string;
  count: string;
  angle: string;
  offsetAngle: string;
}

const trimmed = (value: number, decimals: number): string => String(Number(value.toFixed(decimals)));

/** A length as an entry shows it: the number in the display units, without trailing noise. */
export function lengthEntry(um: number, units: LengthUnit): string {
  return trimmed(umTo(um, units), units === "mm" ? 4 : units === "mil" ? 2 : 5);
}

/** `RestoreConfigToControls`: the options as the entries show them. With "Full circle" on the angle entry shows the division (`calculateCircularArrayProperties`). */
export function entriesOf(o: ArrayOptions, units: LengthUnit): ArrayEntries {
  return {
    nx: String(o.nx),
    ny: String(o.ny),
    dx: lengthEntry(o.dx, units),
    dy: lengthEntry(o.dy, units),
    offsetX: lengthEntry(o.offsetX, units),
    offsetY: lengthEntry(o.offsetY, units),
    stagger: String(o.stagger),
    centerX: lengthEntry(o.centerX, units),
    centerY: lengthEntry(o.centerY, units),
    count: String(o.count),
    angle: trimmed(effectiveAngleDeg(o), 4),
    offsetAngle: trimmed(o.offsetAngleDeg, 4),
  };
}

export type ArrayParse = { ok: true; options: ArrayOptions } | { ok: false; error: string };

/**
 * `TransferDataFromWindow`: the entries of the current tab read against `base` (the flags the radios and check boxes hold, and the previous numbers, which stay for
 * the tab that is not shown). A count or the stagger must be a whole number (`validateLongEntry`: "Bad numeric value for horizontal count: x"); a length takes a unit
 * suffix and means the display units without one; an angle may end with a degree sign. All the complaints are returned together, one to a line.
 */
export function optionsFromEntries(base: ArrayOptions, e: ArrayEntries, units: LengthUnit): ArrayParse {
  const errors: string[] = [];
  const quiet: string[] = [];
  const shown = (tab: ArrayTab): string[] => (base.tab === tab ? errors : quiet);
  const long = (into: string[], text: string, what: string, keep: number): number => {
    if (/^\s*[-+]?\d+\s*$/.test(text)) return parseInt(text, 10);
    into.push(`Bad numeric value for ${what}: ${text}`);
    return keep;
  };
  const length = (into: string[], text: string, what: string, keep: number): number => {
    const p = parseValue({ kind: "int", display: "size" }, text, units);
    if (p.ok && typeof p.value === "number") return p.value;
    into.push(`Bad numeric value for ${what}: ${text}`);
    return keep;
  };
  const degrees = (into: string[], text: string, what: string, keep: number): number => {
    const p = parseValue({ kind: "double", display: "degree" }, text, units);
    if (p.ok && typeof p.value === "number") return p.value;
    into.push(`Bad numeric value for ${what}: ${text}`);
    return keep;
  };
  const g = shown("grid");
  const c = shown("circular");
  const o: ArrayOptions = {
    ...base,
    nx: long(g, e.nx, "horizontal count", base.nx),
    ny: long(g, e.ny, "vertical count", base.ny),
    dx: length(g, e.dx, "horizontal spacing", base.dx),
    dy: length(g, e.dy, "vertical spacing", base.dy),
    offsetX: length(g, e.offsetX, "horizontal offset", base.offsetX),
    offsetY: length(g, e.offsetY, "vertical offset", base.offsetY),
    stagger: long(g, e.stagger, "stagger", base.stagger),
    centerX: length(c, e.centerX, "center pos X", base.centerX),
    centerY: length(c, e.centerY, "center pos Y", base.centerY),
    count: long(c, e.count, "point count", base.count),
    angleDeg: base.fullCircle ? base.angleDeg : degrees(c, e.angle, "angle between items", base.angleDeg),
    offsetAngleDeg: degrees(c, e.offsetAngle, "first item angle", base.offsetAngleDeg),
  };
  if (errors.length === 0) {
    const bad = checkArrayOptions(o);
    if (bad) errors.push(bad);
  }
  return errors.length > 0 ? { ok: false, error: errors.join("\n") } : { ok: true, options: o };
}

/** What the dialog keeps for the next time (`ReadConfigFromControls`): the options as they were used, the angle as the entry held it. */
export function rememberedOptions(o: ArrayOptions): ArrayOptions {
  return { ...o, angleDeg: effectiveAngleDeg(o) };
}

// ------------------------------------------------------------------------------------------------------------------- what they make

/** `ARRAY_GRID_OPTIONS` / `ARRAY_CIRCULAR_OPTIONS` for the current tab. */
export function arrayGeometry(o: ArrayOptions): ArrayGeometry {
  if (o.tab === "grid") {
    return {
      kind: "grid",
      nx: o.nx,
      ny: o.ny,
      dx: Math.round(o.dx),
      dy: Math.round(o.dy),
      offset_x: Math.round(o.offsetX),
      offset_y: Math.round(o.offsetY),
      centred: o.centred,
      stagger: o.stagger,
      stagger_rows: o.staggerRows,
      horizontal_then_vertical: !o.verticalFirst,
      reverse_alternate: o.reverseAlternate,
    };
  }
  return {
    kind: "circular",
    center: { x: Math.round(o.centerX), y: Math.round(o.centerY) },
    count: o.count,
    angle_millideg: Math.round(effectiveAngleDeg(o) * 1000),
    angle_offset_millideg: Math.round(o.offsetAngleDeg * 1000),
    clockwise: o.clockwise,
    rotate_items: o.rotateItems,
  };
}

/** The one command a Create Array makes. */
export function arrayCmd(o: ArrayOptions, ids: readonly string[]): Cmd {
  return { op: "create_array", ids: [...ids], geometry: arrayGeometry(o), arrange: o.arrange, reannotate: o.reannotate };
}

type Box = readonly [number, number, number, number];

/**
 * `ARRAY_TOOL::CreateArray`'s `origin`: a lone item's position, else the centre of the box around the selection (`PCB_SELECTION::GetCenter`). `positionOf` and
 * `boundsOf` are the board's `BOARD_ITEM::GetPosition()` and `GetBoundingBox()` of an id; null when there is no item to look at.
 */
export function arrayOrigin(ids: readonly string[], positionOf: (id: string) => readonly [number, number] | null, boundsOf: (id: string) => Box | null): [number, number] | null {
  if (ids.length === 0) return null;
  if (ids.length === 1) {
    const at = positionOf(ids[0]!);
    return at ? [at[0], at[1]] : null;
  }
  const boxes = ids.map(boundsOf).filter((b): b is Box => b !== null);
  if (boxes.length === 0) return null;
  const x0 = Math.min(...boxes.map((b) => b[0]));
  const y0 = Math.min(...boxes.map((b) => b[1]));
  const x1 = Math.max(...boxes.map((b) => b[2]));
  const y1 = Math.max(...boxes.map((b) => b[3]));
  return [Math.round((x0 + x1) / 2), Math.round((y0 + y1) / 2)];
}

/**
 * Where the grid's points are for an item at the origin, as `ARRAY_OPTIONS::GetTransform` (crates/ops/src/array.rs `ArrayGeometry::transform`) places them, in the
 * order the grid is numbered: for the dialog's preview of how many points there are and where. Grid only.
 */
export function gridPoints(o: Pick<ArrayOptions, "nx" | "ny" | "dx" | "dy" | "offsetX" | "offsetY" | "stagger" | "staggerRows" | "centred" | "verticalFirst" | "reverseAlternate">): Array<[number, number]> {
  const out: Array<[number, number]> = [];
  if (!(o.nx >= 1 && o.ny >= 1) || !Number.isInteger(o.nx) || !Number.isInteger(o.ny)) return out;
  const total = o.nx * o.ny;
  const horizontalFirst = !o.verticalFirst;
  const axis = Math.max(1, horizontalFirst ? o.nx : o.ny);
  for (let n = 0; n < total; n++) {
    let cx = n % axis;
    let cy = Math.floor(n / axis);
    if (o.reverseAlternate && cy % 2 === 1) cx = axis - cx - 1;
    if (!horizontalFirst) [cx, cy] = [cy, cx];
    let x = cx * o.dx + cy * o.offsetX;
    let y = cy * o.dy + cx * o.offsetY;
    if (Math.abs(o.stagger) > 1) {
      const s = Math.abs(o.stagger);
      const idx = (o.staggerRows ? cy : cx) % s;
      const signed = idx * Math.sign(o.stagger);
      x += Math.trunc(((o.staggerRows ? o.dx : o.offsetX) * signed) / s);
      y += Math.trunc(((o.staggerRows ? o.offsetY : o.dy) * signed) / s);
    }
    if (o.centred) {
      x -= Math.trunc(((o.nx - 1) * o.dx + (o.ny - 1) * o.offsetX) / 2);
      y -= Math.trunc(((o.ny - 1) * o.dy + (o.nx - 1) * o.offsetY) / 2);
    }
    out.push([x, y]);
  }
  return out;
}

/**
 * The points an item at `origin` lands on, absolute: the grid's offsets from it, or the item turned about the centre by the angle times the index (plus the first
 * item's angle), clockwise on the screen when `clockwise` -- `GetTransform` for both. For the dialog's preview; empty for options that make nothing.
 */
export function previewPoints(o: ArrayOptions, origin: readonly [number, number]): Array<[number, number]> {
  if (checkArrayOptions(o) !== null) return [];
  if (o.tab === "grid") return gridPoints(o).map(([x, y]) => [origin[0] + x, origin[1] + y]);
  const out: Array<[number, number]> = [];
  const step = effectiveAngleDeg(o);
  for (let n = 0; n < o.count; n++) {
    const deg = (step === 0 ? (360 * n) / o.count : step * n) + o.offsetAngleDeg;
    const a = ((o.clockwise ? deg : -deg) * Math.PI) / 180;
    const rx = origin[0] - o.centerX;
    const ry = origin[1] - o.centerY;
    out.push([o.centerX + rx * Math.cos(a) - ry * Math.sin(a), o.centerY + rx * Math.sin(a) + ry * Math.cos(a)]);
  }
  return out;
}
