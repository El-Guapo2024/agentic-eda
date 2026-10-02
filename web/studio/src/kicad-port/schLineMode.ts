// Port of the wire/bus "Line Mode" machinery in eeschema:
//   - LINE_MODE enum                     (eeschema/eeschema_settings.h)
//   - SCH_EDITOR_CONTROL::NextLineMode   (eeschema/tools/sch_editor_control.cpp,
//     `Shift+Space`: line_mode = (line_mode + 1) % LINE_MODE_COUNT)
//   - SCH_LINE_WIRE_BUS_TOOL::computeBreakPoint
//     (eeschema/tools/sch_line_wire_bus_tool.cpp) -- where the two-segment
//     "elbow" between the last clicked point and the cursor is placed; the
//     `/` hotkey (SCH_ACTIONS::switchSegmentPosture, handled in
//     doDrawSegments) just flips the `posture` boolean fed to it.
// Pure: no state, no DOM.

export const LINE_MODE_FREE = 0;
export const LINE_MODE_90 = 1;
export const LINE_MODE_45 = 2;
export const LINE_MODE_COUNT = 3;
export type LineMode = 0 | 1 | 2;

/** SCH_EDITOR_CONTROL::NextLineMode. */
export function nextLineMode(mode: LineMode): LineMode {
  return ((mode + 1) % LINE_MODE_COUNT) as LineMode;
}

export type Pt = readonly [number, number];
export interface Seg {
  start: Pt;
  end: Pt;
}

/**
 * computeBreakPoint: given the segment pair currently being previewed
 * (`segment`: last click -> previous break point, `nextSegment`: previous
 * break point -> previous cursor) returns the new break point for `cursor`.
 * The previous pair only supplies the "maintain current line shape" hint
 * (preferHorizontal/preferVertical); pass zero-length segments for a fresh
 * start. Sheet-pin forced-horizontal handling is not ported (no sheet pins
 * in this IR).
 */
export function computeBreakPoint(segment: Seg, nextSegment: Seg, cursor: Pt, mode: LineMode, posture: boolean): Pt {
  const sx = segment.start[0];
  const sy = segment.start[1];
  const dx = cursor[0] - sx;
  const dy = cursor[1] - sy;
  const xDir = dx > 0 ? 1 : -1;
  const yDir = dy > 0 ? 1 : -1;

  let preferHorizontal: boolean;
  let preferVertical: boolean;
  if (mode === LINE_MODE_45 && posture) {
    preferHorizontal = nextSegment.end[0] - nextSegment.start[0] !== 0;
    preferVertical = nextSegment.end[1] - nextSegment.start[1] !== 0;
  } else {
    preferHorizontal = segment.end[0] - segment.start[0] !== 0;
    preferVertical = segment.end[1] - segment.start[1] !== 0;
  }

  let mx = 0;
  let my = 0;
  const breakVertical = () => {
    if (mode === LINE_MODE_45) {
      if (!posture) {
        mx = sx;
        my = cursor[1] - yDir * Math.abs(dx);
      } else {
        mx = cursor[0];
        my = sy + yDir * Math.abs(dx);
      }
    } else {
      mx = sx;
      my = cursor[1];
    }
  };
  const breakHorizontal = () => {
    if (mode === LINE_MODE_45) {
      if (!posture) {
        mx = cursor[0] - xDir * Math.abs(dy);
        my = sy;
      } else {
        mx = sx + xDir * Math.abs(dy);
        my = cursor[1];
      }
    } else {
      mx = cursor[0];
      my = sy;
    }
  };

  if (preferVertical) breakVertical();
  else if (preferHorizontal) breakHorizontal();

  const dmx = mx - sx;
  const dmy = my - sy;
  const signbit = (v: number) => v < 0;
  if (mode === LINE_MODE_45 && !posture && (signbit(dmx) !== signbit(dx) || signbit(dmy) !== signbit(dy))) {
    preferVertical = false;
    preferHorizontal = false;
  } else if (mode === LINE_MODE_45 && posture && (Math.abs(dmx) > Math.abs(dx) || Math.abs(dmy) > Math.abs(dy))) {
    preferVertical = false;
    preferHorizontal = false;
  }

  if (!preferHorizontal && !preferVertical) {
    if (Math.abs(dx) < Math.abs(dy)) breakVertical();
    else breakHorizontal();
  }
  return [mx, my];
}

/**
 * The points a wire being drawn from `last` to `cursor` should pass
 * through: in LINE_MODE_FREE a single straight segment, otherwise two
 * segments via computeBreakPoint (doDrawSegments' `twoSegments`). `prev` is
 * the previous preview's break point/cursor (for the shape hint), or null.
 */
export function wireTail(last: Pt, cursor: Pt, mode: LineMode, posture: boolean, prev: { mid: Pt; end: Pt } | null): Pt[] {
  if (mode === LINE_MODE_FREE) return [cursor];
  const seg: Seg = { start: last, end: prev?.mid ?? last };
  const next: Seg = { start: prev?.mid ?? last, end: prev?.end ?? last };
  const mid = computeBreakPoint(seg, next, cursor, mode, posture);
  // Drop a degenerate elbow (coincident with an endpoint).
  if ((mid[0] === last[0] && mid[1] === last[1]) || (mid[0] === cursor[0] && mid[1] === cursor[1])) return [cursor];
  return [mid, cursor];
}
