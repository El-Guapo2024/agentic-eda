// Create Array's options and what the dialog does with them: the defaults KiCad's dialog starts from, the text entries and how they are read
// (`DIALOG_CREATE_ARRAY::TransferDataFromWindow`), the geometry (`ARRAY_GRID_OPTIONS` / `ARRAY_CIRCULAR_OPTIONS`) and the one command (`create_array`)
// they make, and where the circular array's centre starts (`ARRAY_TOOL::CreateArray`'s `origin`: a lone item's position, else the centre of the selection).
//
// Ported from pcbnew/dialogs/dialog_create_array.cpp (`CREATE_ARRAY_DIALOG_ENTRIES`, `TransferDataFromWindow`, `validateLongEntry`,
// `calculateCircularArrayProperties`) and pcbnew/tools/array_tool.cpp at KiCad 8303b2ad; the backend half is crates/ops/src/array.rs.
//
// The footprint editor's half of the dialog -- the "Pad numbering" panels, shown there only (`enableArrayNumbering = m_isFootprintEditor`) -- is `PadNumberingOptions`
// below, `padNumberingOf` (the dialog's check of the axis entries) and `fpArrayCmd` (the one command, `create_footprint_array`); the numbers themselves are
// kicad-port/arrayAxis.ts (`ARRAY_AXIS`) and crates/ops/src/array_numbering.rs, `fp_array.rs`.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { ArrayGeometry, Cmd, LibraryPad, PadNumbering, Um } from "../api/types";
import { umTo, type LengthUnit } from "../state/units";
import { axisOf, providedNumbers, resolveNumbering, type AxisSpec, type NumberedArray, type NumberingType } from "./arrayAxis";
import { parseValue } from "./propertyGrid";

export type ArrayTab = "grid" | "circular";

/** One numbering axis as the dialog's controls hold it: the choice of alphabet and the two text entries ("Pad numbering start", "Pad numbering increment"). */
export interface AxisEntry {
  kind: NumberingType;
  start: string;
  step: string;
}

/** The footprint editor's "Pad numbering" panels (`m_gridPadNumberingPanel`, `m_circularPadNumberingPanel`). */
export interface PadNumberingOptions {
  /** `m_cbRenumberPads`: a grid renumbers its pads or leaves the copies the number of their source. (A circle always renumbers: `SetShouldNumber( m_isFootprintEditor )`.) */
  renumber: boolean;
  /** `m_rbGridStartNumberingOpt`: false is "Use first free number", true "From start value". */
  gridFromStart: boolean;
  /** `m_radioBoxGridNumberingScheme`: false is "Continuous (1, 2, 3...)", true "Coordinate (A1, A2, ... B1, ...)". */
  gridCoordinates: boolean;
  /** "Primary axis numbering" with its start and increment, and the "Secondary axis numbering" of a coordinate scheme. */
  primary: AxisEntry;
  secondary: AxisEntry;
  /** `m_rbCircStartNumberingOpt`, and the circle's one axis (`m_choiceCircNumbering`, `m_entryCircNumberingStart`, `m_entryCircNumberingStep`). */
  circFromStart: boolean;
  circular: AxisEntry;
}

const AXIS_FROM_ONE: AxisEntry = { kind: "numeric", start: "1", step: "1" };
const AXIS_FROM_ONE_SPEC: AxisSpec = { kind: "numeric", start: "1", step: 1 };

/** `CREATE_ARRAY_DIALOG_ENTRIES`: renumber, from the start value 1, one continuous run of numerals, step 1, on both tabs. */
export const DEFAULT_PAD_NUMBERING: PadNumberingOptions = {
  renumber: true,
  gridFromStart: true,
  gridCoordinates: false,
  primary: AXIS_FROM_ONE,
  secondary: AXIS_FROM_ONE,
  circFromStart: true,
  circular: AXIS_FROM_ONE,
};

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
  /** The footprint editor's pad numbering; the board's dialog never shows it. */
  pads: PadNumberingOptions;
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
  pads: DEFAULT_PAD_NUMBERING,
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

// ------------------------------------------------------------------------------------------------------------------- the footprint editor's pad numbering

/**
 * `TransferDataFromWindow`'s numbering half, for the tab being used: the `PadNumbering` the entries make -- null when the grid's "Renumber pads" is off, so the copies
 * keep their source's number -- or the dialog's complaints, every one on a line of its own. A start is judged against its alphabet (`validateAxisOptions`), and a step
 * must be a whole number (`validateLongEntry`: "Bad numeric value for step value: x"); the entries of "Use first free number" and of the axes a scheme does not use are
 * not looked at.
 */
export function padNumberingOf(o: ArrayOptions): { ok: true; numbering: PadNumbering | null } | { ok: false; error: string } {
  const circular = o.tab === "circular";
  const p = o.pads;
  if (!circular && !p.renumber) return { ok: true, numbering: null };
  const fromStart = circular ? p.circFromStart : p.gridFromStart;
  // "Use first free number" is an artificial linear scheme (`newGrid->m_2dArrayNumbering = false`), whatever the scheme radio says
  const twoD = !circular && fromStart && p.gridCoordinates;
  const errors: string[] = [];
  /** `validateAxisOptions`: the start is judged first, and only a start that decodes gets its step looked at. */
  const axisSpec = (entry: AxisEntry): AxisSpec => {
    const start = axisOf({ kind: entry.kind, start: entry.start, step: 1 });
    if (!start.ok) {
      errors.push(start.error);
      return { kind: entry.kind, start: entry.start, step: 1 };
    }
    if (!/^\s*[-+]?\d+\s*$/.test(entry.step)) {
      errors.push(`Bad numeric value for step value: ${entry.step}`);
      return { kind: entry.kind, start: entry.start, step: 1 };
    }
    return { kind: entry.kind, start: entry.start, step: parseInt(entry.step, 10) | 0 };
  };
  const numbering: PadNumbering = { start_specified: fromStart, two_d: twoD, primary: { ...AXIS_FROM_ONE_SPEC }, secondary: { ...AXIS_FROM_ONE_SPEC } };
  if (fromStart) {
    numbering.primary = axisSpec(circular ? p.circular : p.primary);
    if (twoD) numbering.secondary = axisSpec(p.secondary);
  }
  return errors.length > 0 ? { ok: false, error: errors.join("\n") } : { ok: true, numbering };
}

/** The one command a Create Array makes in the Footprint Editor: `ids` are the selected pads, graphics and texts of `footprint`. */
export function fpArrayCmd(o: ArrayOptions, footprint: string, ids: readonly string[]): { ok: true; cmd: Cmd } | { ok: false; error: string } {
  const numbering = padNumberingOf(o);
  if (!numbering.ok) return numbering;
  // "Arrange selection" returns before the numbering: nothing is renumbered
  return { ok: true, cmd: { op: "create_footprint_array", footprint, ids: [...ids], geometry: arrayGeometry(o), arrange: o.arrange, numbering: o.arrange ? null : numbering.numbering } };
}

/** The shape of the array as the numbering reads it. */
export function numberedArrayOf(o: ArrayOptions): NumberedArray {
  return { circular: o.tab === "circular", nx: o.nx, ny: o.ny, horizontalThenVertical: !o.verticalFirst, reverseAlternate: o.reverseAlternate };
}

/**
 * The numbers the pads of the array will take, as far as they can be told: the first `limit` of `padsPerPoint * points`, with the numbers of the pads that stay
 * (`unchangingPadNumbers`: every pad that is not selected) reserved. Empty when the numbering is off, the entries do not make one, or the array makes nothing.
 */
export function previewPadNumbers(o: ArrayOptions, pads: readonly LibraryPad[], selected: ReadonlySet<string>, limit: number): string[] {
  if (o.arrange || checkArrayOptions(o) !== null) return [];
  const numbering = padNumberingOf(o);
  if (!numbering.ok || !numbering.numbering) return [];
  const resolved = resolveNumbering(numbering.numbering, o.tab === "circular");
  if (!resolved.ok) return [];
  const numberable = pads.filter((p) => p.id && selected.has(p.id) && p.kind !== "non_plated_hole").length;
  const points = o.tab === "grid" ? o.nx * o.ny : o.count;
  const total = numberable * points;
  if (total === 0) return [];
  const reserved = new Set(pads.filter((p) => !p.id || !selected.has(p.id)).map((p) => p.number));
  return providedNumbers(numberedArrayOf(o), resolved.axes, reserved, Math.min(limit, total));
}
