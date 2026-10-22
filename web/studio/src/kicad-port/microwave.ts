// The microwave tools' pure half (`pcbnew/microwave/microwave_tool.cpp`, `microwave_footprint.cpp`, `microwave_inductor.cpp`, `microwave_polygon.cpp` at KiCad 8303b2ad): the words of
// their dialogs, what the dialogs' answers are read as, and where the inductor's end point is held. The footprints themselves are built by the server
// (`crates/ops/src/microwave.rs`, one verb each). Unit-tested in microwave.test.ts.
import type { PointXY } from "../api/types";
import type { LengthUnit } from "../state/units";
import { umFrom, umTo } from "../state/units";
import type { AngleSnapMode } from "./pcbParityState";
import { snapVector45 } from "./polygonGeom";

/** The title every one of the tools' text prompts has (`_( "Create Microwave Footprint" )`). */
export const MICROWAVE_TITLE = "Create Microwave Footprint";

/** The tools that ask one size: `createFootprint`'s `msg`. */
export type SizedTool = "gap" | "stub" | "arc_stub";
export const SIZE_LABEL: Record<SizedTool, string> = { gap: "Gap Size:", stub: "Stub Size:", arc_stub: "Arc Stub Radius Value:" };

/** The arc stub's second prompt: `msg = wxT( "0.0" )` in `WX_TEXT_ENTRY_DIALOG angledlg( editFrame, _( "Angle in degrees:" ), .. )`. */
export const ANGLE_LABEL = "Angle in degrees:";
export const ANGLE_DEFAULT = "0.0";

/** The inductor's two prompts: "Length of track:" (starting at the distance between the points) and "Component value:" (starting at `L`). */
export const LENGTH_LABEL = "Length of track:";
export const VALUE_LABEL = "Component value:";
export const VALUE_DEFAULT = "L";

/** What `DisplayError( editFrame, _( "Incorrect number, abort" ) )` says when the angle is not a number. */
export const INCORRECT_NUMBER = "Incorrect number, abort";

/** `wxString::ToDouble`: the whole text is one number (blanks before it are fine, after it are not), `null` when it is not. */
export function parseDouble(text: string): number | null {
  const m = /^\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)$/.exec(text);
  if (!m) return null;
  const v = Number(m[1]);
  return Number.isFinite(v) ? v : null;
}

/**
 * The arc stub's angle once typed: `angle = EDA_ANGLE( fval, DEGREES_T ); if( angle < ANGLE_0 ) angle = -angle; if( angle > ANGLE_180 ) angle = ANGLE_180;`, in the millidegrees the verb takes
 * (the server clamps again).
 */
export function arcStubAngleMillideg(degrees: number): number {
  return Math.round(Math.min(Math.abs(degrees), 180) * 1000);
}

/** `FOOTPRINT_NAME_VALIDATOR`: the characters a footprint's name cannot have (`%$<>\t\n\r"\/:`). */
export const ILLEGAL_NAME_CHARS = "%$<>\t\n\r\"\\/:";

/** The "Component value:" box accepts a text that is not empty and has none of [`ILLEGAL_NAME_CHARS`]. */
export function isValidValueName(text: string): boolean {
  return text.length > 0 && ![...text].some((c) => ILLEGAL_NAME_CHARS.includes(c));
}

/**
 * `TWO_POINT_GEOMETRY_MANAGER::SetEnd` with the tool's angle snap (`tpGeomMgr.SetAngleSnap( GetAngleSnapMode() )`): the end of the line is the cursor put on the nearest 45 degree
 * ray (`GetVectorSnapped45`) or the nearest axis (`GetVectorSnapped90`) from the start, the way KiCad snaps it, or left where it is when the line is free.
 */
export function inductorEnd(start: PointXY, cursor: PointXY, mode: AngleSnapMode): PointXY {
  const v: [number, number] = [cursor.x - start.x, cursor.y - start.y];
  if (mode === "45") {
    const s = snapVector45(v);
    return { x: start.x + s[0], y: start.y + s[1] };
  }
  if (mode === "90") {
    return Math.abs(v[0]) >= Math.abs(v[1]) ? { x: start.x + v[0], y: start.y } : { x: start.x, y: start.y + v[1] };
  }
  return { x: cursor.x, y: cursor.y };
}

/** `pt.EuclideanNorm()` of the two points: the shortest the coil can be, micrometres. */
export function inductorMinLengthUm(start: PointXY, end: PointXY): number {
  return Math.hypot(end.x - start.x, end.y - start.y);
}

const DECIMALS: Record<LengthUnit, number> = { mm: 4, mil: 2, in: 5 };

/**
 * `StringFromValue( min_len )`, the "Length of track:" box's start, but never below the distance: KiCad rounds the text to the units' digits, so the box it opens would be refused
 * ("Requested length < minimum length") whenever the rounding went down; this one rounds up.
 */
export function lengthTextAtLeast(valueUm: number, units: LengthUnit): string {
  const scale = 10 ** DECIMALS[units];
  return String(Math.ceil(umTo(valueUm, units) * scale - 1e-9) / scale);
}

/** `StringFromValue( value )`: a length in the display units without their name, to the digits the units show. */
export function lengthText(valueUm: number, units: LengthUnit): string {
  return String(Number(umTo(valueUm, units).toFixed(DECIMALS[units])));
}

/** `ValueFromString`: "1.5", "0.2mm", "10mil", `0.01"` as micrometres (a bare number is in the display units); 0 for text that is not a length, as KiCad reads it. */
export function lengthFromText(text: string, units: LengthUnit): number {
  const m = /^\s*(-?\d*\.?\d+)\s*(mm|mil|in|")?\s*$/i.exec(text);
  if (!m) return 0;
  const unit = m[2] ? (m[2] === '"' ? "in" : (m[2].toLowerCase() as LengthUnit)) : units;
  return Math.round(umFrom(parseFloat(m[1]!), unit));
}

/** What the Complex Shape dialog's Size boxes start with once a file is read: `m_sizeX->SetValue( (int) g_ShapeScaleX )`, the file's scale in nanometres shown in the display units. */
export function shapeSizeText(nanometres: number, units: LengthUnit): string {
  return lengthText(nanometres / 1000, units);
}

/**
 * What the dialog's Size box sends: nothing while it still shows the file's own scale (the server uses the exact one), or the length typed in micrometres once it was edited.
 */
export function shapeSizeOverride(text: string, shown: string, units: LengthUnit): number | null {
  return text.trim() === shown.trim() ? null : lengthFromText(text, units);
}

/**
 * `getRectangleAlongCentreLine` (`common/preview_items/centreline_rect_item.cpp`): the four corners of the rectangle the tool previews between its two clicks -- a rectangle along the
 * centre line from `start` to `end` whose width is `aspect` times the line's length (`inductorAreaAspect`, 0.5 in `drawMicrowaveInductor`). A line of no length is taken as one unit long.
 */
export function centrelineRect(start: PointXY, end: PointXY, aspect: number): [number, number][] {
  let cl: [number, number] = [end.x - start.x, end.y - start.y];
  if (cl[0] === 0 && cl[1] === 0) cl = [1, 0];
  // the side is the centre line turned by -90 degrees (`RotatePoint( side, -ANGLE_90 )`: x' = -y, y' = x) and scaled
  const side: [number, number] = [-cl[1] * aspect, cl[0] * aspect];
  let pt: [number, number] = [start.x + side[0] / 2, start.y + side[1] / 2];
  const out: [number, number][] = [pt];
  pt = [pt[0] + cl[0], pt[1] + cl[1]];
  out.push(pt);
  pt = [pt[0] - side[0], pt[1] - side[1]];
  out.push(pt);
  pt = [pt[0] - cl[0], pt[1] - cl[1]];
  out.push(pt);
  return out;
}
