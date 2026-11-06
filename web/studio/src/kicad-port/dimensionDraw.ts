// The board's dimensions as KiCad draws and reshapes them (UI-LEFT P13), from pcbnew at 8303b2ad:
//
//   DRAWING_TOOL::DrawDimension (tools/drawing_tool.cpp)   the click sequences and the live preview: SET_ORIGIN, SET_END, and SET_HEIGHT for an aligned or
//                                                          orthogonal dimension -- kicad-port/fpDrawTools.ts (`fpDimensionClick`, `fpDimensionMotion`), which
//                                                          the Footprint Editor runs too, with the board's dimension style here (`boardDimensionStyle`)
//   PCB_POINT_EDITOR's dimension behaviours (tools/pcb_point_editor.cpp: ALIGNED_DIMENSION_POINT_EDIT_BEHAVIOR, DIM_CENTER_POINT_EDIT_BEHAVIOR,
//   DIM_RADIAL_POINT_EDIT_BEHAVIOR, DIM_LEADER_POINT_EDIT_BEHAVIOR, DIM_ALIGNED_TEXT_UPDATER)
//                                                          the handles of a selected dimension and what dragging one does: the feature points, the crossbar
//                                                          ends (the height), a radial's knee (its leader length) and the text -- which, dragged on an aligned
//                                                          or orthogonal dimension, is placed by hand from then on (`DIM_TEXT_POSITION::MANUAL`)
//
// The geometry of a dimension is the server's (`POST /api/dimension/preview`, `eda_connectivity::dimension`); this module needs only the crossbar of an aligned
// or orthogonal one, which `DIM_ALIGNED_TEXT_UPDATER` compares before and after a change. Not ported: the text justification flip of a left- or right-justified
// text that passes the crossbar's middle (the IR has no justification for a dimension's text, it is always centred).
//
// Pure: no store, no DOM. Unit-tested in dimensionDraw.test.ts.
import type { BoardState, CmdDimension, Dimension } from "../api/types";
import { toCmdDimension } from "./dimensionConvert";
import { FP_DIMENSION_DEFAULTS, radialKnee, type DimensionEditorStyle } from "./fpDrawTools";
import { snapVector45 } from "./polygonGeom";

type P = [number, number];

/**
 * The board's dimension style (`setMeasurementAttributes`' `boardSettings.m_Dimension*`: Board Setup > Dimension Properties; the arrows and the extension offset;
 * the layers' Text & Graphics defaults for the text and the line), as `fpDimensionClick` takes it.
 */
export function boardDimensionStyle(board: Pick<BoardState, "drawings" | "board_rules"> | null | undefined): DimensionEditorStyle {
  const s = board?.drawings?.dimension_settings;
  return {
    settings: s
      ? {
          units: s.units,
          precision: s.precision,
          units_format: s.units_format,
          suppress_trailing_zeros: s.suppress_trailing_zeros,
          // `PARAM_ENUM<DIM_TEXT_POSITION>( .., OUTSIDE, INLINE )`: a board default is never manual
          text_position: s.text_position === "manual" ? "outside" : s.text_position,
          keep_text_aligned: s.keep_text_aligned,
          arrow_length: s.arrow_length,
          extension_offset: s.extension_offset,
        }
      : FP_DIMENSION_DEFAULTS,
    textDefaults: board?.board_rules?.text_graphics,
  };
}

// ------------------------------------------------------------------------------------------------------------------------- point editor

/** `DIMENSION_POINTS`: DIM_START, DIM_END, DIM_TEXT, DIM_CROSSBARSTART, DIM_CROSSBAREND (an aligned or orthogonal one), DIM_KNEE (a radial one). */
export type DimPointId = "start" | "end" | "text" | "crossbar_start" | "crossbar_end" | "knee";

export interface DimEditPoint {
  id: DimPointId;
  pos: P;
}

/** The point editor's handles of a dimension (`MakePoints`): a centre mark has its two points, a leader its text too, a radial dimension its knee, an aligned or orthogonal one its crossbar ends. */
export function dimensionEditPoints(d: Dimension): DimEditPoint[] {
  const out: DimEditPoint[] = [
    { id: "start", pos: [d.start[0], d.start[1]] },
    { id: "end", pos: [d.end[0], d.end[1]] },
  ];
  if (d.kind === "center") return out;
  out.push({ id: "text", pos: [d.text_at[0], d.text_at[1]] });
  if ((d.kind === "aligned" || d.kind === "orthogonal") && d.crossbar) {
    out.push({ id: "crossbar_start", pos: [d.crossbar[0][0], d.crossbar[0][1]] }, { id: "crossbar_end", pos: [d.crossbar[1][0], d.crossbar[1][1]] });
  }
  if (d.kind === "radial") out.push({ id: "knee", pos: d.knee ? [d.knee[0], d.knee[1]] : radialKnee(toCmdDimension(d), d.leader_length ?? 0) });
  return out;
}

/** The handle under `at`, within `tolerance` (the nearest; `EDIT_POINTS::FindPoint`). */
export function dimensionPointAt(d: Dimension, at: P, tolerance: number): DimPointId | null {
  let best: DimPointId | null = null;
  let bestDist = tolerance;
  for (const p of dimensionEditPoints(d)) {
    const dist = Math.hypot(p.pos[0] - at[0], p.pos[1] - at[1]);
    if (dist <= bestDist) {
      best = p.id;
      bestDist = dist;
    }
  }
  return best;
}

/** `KiROUND`, no negative zero. */
const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v)) + 0;
const sub = (a: P, b: P): P => [a[0] - b[0], a[1] - b[1]];
const add = (a: P, b: P): P => [a[0] + b[0], a[1] + b[1]];

/** `VECTOR2I::Resize`: `v` at length `len` (the zero vector stays zero). */
function resize(v: P, len: number): P {
  const n = Math.hypot(v[0], v[1]);
  if (n === 0) return [0, 0];
  return [kiRound((v[0] * len) / n), kiRound((v[1] * len) / n)];
}

/**
 * The crossbar of an aligned or orthogonal dimension as `updateGeometry` places it: an aligned one's ends are the feature points moved across the measured line
 * by the height (`extension` turned to the height's side, `sign( m_height ) * extension.Resize( m_height )`), an orthogonal one's start is the start moved by the
 * height along its axis and its end the end's x (or y) on that line.
 */
export function crossbarOf(d: CmdDimension): [P, P] | null {
  const s: P = [d.start.x, d.start.y];
  const e: P = [d.end.x, d.end.y];
  if (d.kind.kind === "aligned") {
    const h = d.kind.height;
    const dim = sub(e, s);
    const extension: P = h > 0 ? [-dim[1], dim[0]] : [dim[1], -dim[0]];
    const offset = resize(extension, Math.abs(h));
    return [add(s, offset), add(e, offset)];
  }
  if (d.kind.kind === "orthogonal") {
    const h = d.kind.height;
    const extension: P = d.kind.horizontal ? [0, h] : [h, 0];
    const cs = add(s, resize(extension, Math.abs(h)));
    return [cs, d.kind.horizontal ? [e[0], cs[1]] : [cs[0], e[1]]];
  }
  return null;
}

/** `EDA_ANGLE( VECTOR2I )`, degrees (KiCad's exact axis values). */
function angleOf(v: P): number {
  const [x, y] = v;
  if (x === 0 && y === 0) return 0;
  if (y === 0) return x >= 0 ? 0 : -180;
  if (x === 0) return y >= 0 ? 90 : -90;
  if (x === y) return x >= 0 ? 45 : -135;
  if (x === -y) return x >= 0 ? -45 : 135;
  return (Math.atan2(y, x) * 180) / Math.PI;
}

/** `GetRotated( v, angle )` (`RotatePoint`): `x cos + y sin`, `y cos - x sin` -- counter-clockwise on the screen for a positive angle. */
function rotated(v: P, deg: number): P {
  const r = (deg * Math.PI) / 180;
  const [c, sn] = [Math.cos(r), Math.sin(r)];
  return [kiRound(v[1] * sn + v[0] * c), kiRound(v[1] * c - v[0] * sn)];
}

/** `SEG::NearestPoint`, and the segment parameter of the projection. */
function nearestOnSeg(a: P, b: P, p: P): { pt: P; t: number } {
  const d = sub(b, a);
  const l2 = d[0] * d[0] + d[1] * d[1];
  if (l2 === 0) return { pt: a, t: 0 };
  const t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2;
  const tc = Math.max(0, Math.min(1, t));
  return { pt: [kiRound(a[0] + d[0] * tc), kiRound(a[1] + d[1] * tc)], t };
}

/**
 * `DIM_ALIGNED_TEXT_UPDATER::getDimensionNewTextPosition`: where a text that was at `text` beside the crossbar `old` goes once the crossbar is `now`. Past the
 * crossbar's ends (`!PointProjectsOntoSegment`), it keeps its offset from the nearer end, turned with the crossbar, from whichever new end is on its side; over
 * the crossbar, it keeps its distance across it and its place along it (`GetLengthRatioFromStart`), turned with it.
 */
export function textAfterCrossbarChange(text: P, old: [P, P], now: [P, P]): P {
  if (old[0][0] === now[0][0] && old[0][1] === now[0][1] && old[1][0] === now[1][0] && old[1][1] === now[1][1]) return text;
  const rotation = angleOf(sub(old[1], old[0])) - angleOf(sub(now[1], now[0]));
  const { pt: projected, t } = nearestOnSeg(old[0], old[1], text);
  if (t < 0 || t > 1) {
    const nearEnd = Math.hypot(text[0] - old[0][0], text[1] - old[0][1]) <= Math.hypot(text[0] - old[1][0], text[1] - old[1][1]) ? old[0] : old[1];
    const oldCentre: P = [kiRound((old[0][0] + old[1][0]) / 2), kiRound((old[0][1] + old[1][1]) / 2)];
    const fromCentre = rotated(sub(text, oldCentre), rotation);
    const fromEnd = rotated(sub(text, nearEnd), rotation);
    const newCentre: P = [kiRound((now[0][0] + now[1][0]) / 2), kiRound((now[0][1] + now[1][1]) / 2)];
    // `KIGEOM::PointIsInDirection( crossbarStart, rotTextOffsetFromCbCenter, newCrossBar.Center() )`
    const toStart = sub(now[0], newCentre);
    const startIsThatWay = toStart[0] * fromCentre[0] + toStart[1] * fromCentre[1] > 0;
    return add(startIsThatWay ? now[0] : now[1], fromEnd);
  }
  const oldLen = Math.hypot(old[1][0] - old[0][0], old[1][1] - old[0][1]);
  const ratio = oldLen === 0 ? 0 : Math.hypot(projected[0] - old[0][0], projected[1] - old[0][1]) / oldLen;
  const across = rotated(sub(text, projected), rotation);
  const along: P = [kiRound(now[0][0] + (now[1][0] - now[0][0]) * ratio), kiRound(now[0][1] + (now[1][1] - now[0][1]) * ratio)];
  return add(along, across);
}

/** `EC_LINE`: `p` projected on the line through `through` along `dir` (`projection.IntersectLines( main )`); `p` as it is when the line has no direction. */
function onLine(p: P, through: P, dir: P): P {
  const l2 = dir[0] * dir[0] + dir[1] * dir[1];
  if (l2 === 0) return p;
  const t = ((p[0] - through[0]) * dir[0] + (p[1] - through[1]) * dir[1]) / l2;
  return [kiRound(through[0] + dir[0] * t), kiRound(through[1] + dir[1] * t)];
}

/** `EC_45DEGREE`: `p` on the nearest line of 45 degrees through `from` (`GetVectorSnapped45`). */
function on45(p: P, from: P): P {
  const v = snapVector45(sub(p, from));
  return [from[0] + v[0], from[1] + v[1]];
}

/**
 * The point editor's `UpdateItem` for the handle `id` of `d` dropped (or dragged) at `pos`, with the handle's own constraint applied first: the dimension as
 * `edit_dimension` takes it, one undo step on the drop (`commit.Modify`).
 *
 * - An aligned dimension's crossbar end moves on the line it was on (`EC_LINE( crossbar, feature point )`) and gives the height its length, its side by the cross
 *   product (`featureLine.Cross( crossBar ) > 0`: negative); a feature point moves the point.
 * - An orthogonal one's crossbar end sets the orientation when it leaves the box of the feature points (left of or right of it: vertical; above or below:
 *   horizontal; at a corner by the nearer feature point's offset) and the height across it.
 * - Either one's text is placed by hand (`SetTextPositionMode( DIM_TEXT_POSITION::MANUAL )`); a text placed by hand follows a changed crossbar
 *   (`DIM_ALIGNED_TEXT_UPDATER`).
 * - A centre mark's end stays on a 45-degree line from its centre (`EC_45DEGREE`).
 * - A radial one's end moves the text by as much as the knee moves; its knee moves on the radius (`EC_LINE( start, end )`) and sets the leader length; its text
 *   stays on a 45-degree line from the knee.
 * - A leader's end moves the text with it; its text stays on a 45-degree line from the end.
 */
export function moveDimensionPoint(d: Dimension, id: DimPointId, pos: P): CmdDimension {
  const cmd = toCmdDimension(d);
  const s: P = [cmd.start.x, cmd.start.y];
  const e: P = [cmd.end.x, cmd.end.y];
  const textNow: P = [d.text_at[0], d.text_at[1]];
  const xy = (p: P) => ({ x: kiRound(p[0]), y: kiRound(p[1]) });
  const k = cmd.kind;
  if (k.kind === "aligned" || k.kind === "orthogonal") {
    const before = crossbarOf(cmd);
    if (id === "text") {
      cmd.text_position = "manual";
      cmd.text_pos = xy(pos);
      return cmd;
    }
    if (k.kind === "aligned" && (id === "crossbar_start" || id === "crossbar_end")) {
      const feature = id === "crossbar_start" ? s : e;
      const handle0 = d.crossbar ? (id === "crossbar_start" ? d.crossbar[0] : d.crossbar[1]) : feature;
      const p = onLine(pos, feature, sub([handle0[0], handle0[1]], feature));
      const featureLine = sub(p, feature);
      const crossBar = sub(e, s);
      const len = Math.hypot(featureLine[0], featureLine[1]);
      // `SetHeight( double )` into an int
      k.height = Math.trunc(featureLine[0] * crossBar[1] - featureLine[1] * crossBar[0] > 0 ? -len : len);
    } else if (k.kind === "orthogonal" && (id === "crossbar_start" || id === "crossbar_end")) {
      const [w, h] = [e[0] - s[0], e[1] - s[1]];
      const inside = pos[0] >= Math.min(s[0], e[0]) && pos[0] <= Math.max(s[0], e[0]) && pos[1] >= Math.min(s[1], e[1]) && pos[1] <= Math.max(s[1], e[1]);
      const dA = sub(pos, s);
      const dB = sub(pos, e);
      // `( directionA < directionB ) ? directionA : directionB`: the shorter
      const dir = dA[0] * dA[0] + dA[1] * dA[1] < dB[0] * dB[0] + dB[1] * dB[1] ? dA : dB;
      let vert = !k.horizontal;
      if (!inside) {
        if (w === 0) vert = true;
        else if (h === 0) vert = false;
        else if (pos[0] > s[0] && pos[0] < e[0]) vert = false;
        else if (pos[1] > s[1] && pos[1] < e[1]) vert = true;
        else vert = Math.abs(dir[1]) < Math.abs(dir[0]);
        k.horizontal = !vert;
      }
      const featureLine = sub(pos, s);
      k.height = vert ? kiRound(featureLine[0]) : kiRound(featureLine[1]);
    } else if (id === "start") {
      cmd.start = xy(pos);
    } else if (id === "end") {
      cmd.end = xy(pos);
    }
    // `textPositionUpdater.UpdateTextAfterChange()`: only a text placed by hand keeps a place of its own to move.
    const after = crossbarOf(cmd);
    if (cmd.text_position === "manual" && before && after) cmd.text_pos = xy(textAfterCrossbarChange(cmd.text_pos ? [cmd.text_pos.x, cmd.text_pos.y] : textNow, before, after));
    return cmd;
  }
  if (k.kind === "center") {
    if (id === "start") cmd.start = xy(pos);
    else if (id === "end") cmd.end = xy(on45(pos, s));
    return cmd;
  }
  if (k.kind === "radial") {
    const kneeBefore: P = d.knee ? [d.knee[0], d.knee[1]] : radialKnee(cmd, k.leader_length);
    const text: P = cmd.text_pos ? [cmd.text_pos.x, cmd.text_pos.y] : textNow;
    if (id === "start") {
      cmd.start = xy(pos);
    } else if (id === "end") {
      cmd.end = xy(pos);
      const kneeAfter = radialKnee(cmd, k.leader_length);
      cmd.text_pos = xy(add(text, sub(kneeAfter, kneeBefore)));
    } else if (id === "knee") {
      // `EC_LINE( DIM_START, DIM_END )`: on the radius through the end
      const knee = onLine(pos, e, sub(s, e));
      k.leader_length = kiRound(Math.hypot(knee[0] - e[0], knee[1] - e[1]));
      const kneeAfter = radialKnee(cmd, k.leader_length);
      cmd.text_pos = xy(add(text, sub(kneeAfter, kneeBefore)));
    } else if (id === "text") {
      cmd.text_pos = xy(on45(pos, kneeBefore));
    }
    return cmd;
  }
  // a leader
  const text: P = cmd.text_pos ? [cmd.text_pos.x, cmd.text_pos.y] : textNow;
  if (id === "start") {
    cmd.start = xy(pos);
  } else if (id === "end") {
    cmd.end = xy(pos);
    cmd.text_pos = xy(add(text, sub(pos, e)));
  } else if (id === "text") {
    cmd.text_pos = xy(on45(pos, e));
  }
  return cmd;
}
