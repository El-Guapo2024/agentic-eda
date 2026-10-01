// Global/hierarchical label flag shapes and label text offsets --
// sch_label.cpp's `SCH_GLOBALLABEL::CreateGraphicShape`/
// `SCH_HIERLABEL::CreateGraphicShape`/`GetSchematicTextOffset`, ported
// from exact formulas/lookup tables read from source this session (not
// guessed). All of this operates in this app's shared Y-down um space on
// an already-placed label (`SchematicLabel.at`) -- there is no separate
// "library space" for a label the way there is for a symbol.
//
// KiCad's SPIN_STYLE (which side of the anchor the flag/text sits on) is
// a per-label property in the real file, derived from the label's own
// text angle+justification -- this app's `SchematicLabel` doesn't carry
// one (the coordinator's contract has `scope`/`shape` only), so
// `inferSpin` recovers it from the one wire known to end at the label's
// anchor point, the same geometric-matching approach painter.ts already
// uses for wire junctions. A label with no matching wire end (floating,
// or the "next step" backend placement not landed yet) falls back to
// "right" -- the common case for a label reading left-to-right off a
// wire that approaches from the left.
import type { LabelShape, SchematicWire } from "../../api/types";
import { measureStrokeText } from "../text/strokeFont";

export type LabelSpin = "left" | "up" | "right" | "bottom";

/** eeschema/default_values.h DEFAULT_TEXT_SIZE (50 mil) -- this app's `SchematicLabel` carries no per-label font size, so every label uses KiCad's own factory default. */
export const LABEL_TEXT_SIZE_UM = 1270;

/** KiROUND: round half away from zero (JS's Math.round rounds -0.5 towards +Infinity, i.e. to 0, not -1 -- wrong for KiCad's own convention on negative inputs). Every size/offset formula ported here uses this, matching the real source exactly. */
function kiRound(x: number): number {
  return x >= 0 ? Math.round(x) : -Math.round(-x);
}

/** sch_label.cpp's rotation-by-spin, built on KiCad's own `RotatePoint` (trigo.cpp, Y-down: +90 maps (x,y)->(y,-x)). Spin "left" is the unrotated frame every shape below is authored in. */
function rotateForSpin([x, y]: [number, number], spin: LabelSpin): [number, number] {
  switch (spin) {
    case "left":
      return [x, y];
    case "up":
      return [-y, x]; // -90
    case "right":
      return [-x, -y]; // 180
    case "bottom":
      return [y, -x]; // +90
  }
}

/** Finds the one wire whose first or last point lands on `at` (within a small tolerance) and reports which way it approaches -- used to infer a label's spin, since this app's label data has no explicit orientation of its own. */
function approachDirection(wires: SchematicWire[], at: [number, number]): [number, number] | null {
  const tol = 50; // um -- generous enough for float/rounding slop in generated wire endpoints, tight enough to not match an unrelated nearby point
  for (const w of wires) {
    const pts = w.pts;
    if (pts.length < 2) continue;
    const first = pts[0]!;
    const last = pts[pts.length - 1]!;
    const near = (p: [number, number]) => Math.abs(p[0] - at[0]) <= tol && Math.abs(p[1] - at[1]) <= tol;
    if (near(last)) {
      const prev = pts[pts.length - 2]!;
      return [last[0] - prev[0], last[1] - prev[1]];
    }
    if (near(first)) {
      const next = pts[1]!;
      return [first[0] - next[0], first[1] - next[1]];
    }
  }
  return null;
}

/** See this module's header comment: recovers KiCad's SPIN_STYLE from the wire approaching the label's anchor, defaulting to "right" when none matches. */
export function inferSpin(wires: SchematicWire[], at: [number, number]): LabelSpin {
  const d = approachDirection(wires, at);
  if (!d) return "right";
  const [dx, dy] = d;
  if (Math.abs(dx) >= Math.abs(dy)) return dx >= 0 ? "right" : "left";
  return dy >= 0 ? "bottom" : "up";
}

/** sch_text.cpp SCH_TEXT::GetPenWidth()/GetEffectiveTextPenWidth, not bold, no explicit thickness override (this app's labels carry none): round(size/8), clamped to round(size/4) (never binding here since size/8 < size/4). */
function autoPenWidth(sizeUm: number): number {
  return kiRound(sizeUm / 8);
}

/**
 * The large eeschema global-label flag -- `SCH_GLOBALLABEL::
 * CreateGraphicShape`. Unlike a hierarchical label, its length depends on
 * the actual rendered text (`textBoxW`), so this needs the text content,
 * not just its shape/spin.
 */
export function globalLabelOutline(text: string, shape: LabelShape, spin: LabelSpin, at: [number, number], sizeUm: number = LABEL_TEXT_SIZE_UM): [number, number][] {
  const H = sizeUm;
  const E = kiRound(0.375 * H);
  const pen = autoPenWidth(H);
  const lw = pen;
  // GetTextBox(): (sum of glyph advances - round(0.2*size)) + 2*round(1.5*pen) -- stroke_font.cpp/font.cpp, ported.
  const textBoxW = measureStrokeText(text, H) - kiRound(0.2 * H) + 2 * kiRound(1.5 * pen);
  const L = textBoxW + 2 * E;
  const hs = Math.floor(H / 2) + E;
  const x = L + lw + 3;
  const y = hs + lw + 3;

  let pts: [number, number][];
  switch (shape) {
    case "passive":
      pts = [
        [0, 0],
        [0, -y],
        [-x, -y],
        [-x, 0],
        [-x, y],
        [0, y],
      ];
      break;
    case "output":
      pts = [
        [0, 0],
        [0, -y],
        [-x, -y],
        [-x - hs, 0],
        [-x, y],
        [0, y],
      ];
      break;
    case "input":
      pts = [
        [0, 0],
        [-hs, -y],
        [-x - hs, -y],
        [-x - hs, 0],
        [-x - hs, y],
        [-hs, y],
      ];
      break;
    case "bidirectional":
    case "tri_state":
      pts = [
        [0, 0],
        [-hs, -y],
        [-x - hs, -y],
        [-x - 2 * hs, 0],
        [-x - hs, y],
        [-hs, y],
      ];
      break;
  }
  pts.push(pts[0]!);
  return pts.map((p) => {
    const [rx, ry] = rotateForSpin(p, spin);
    return [rx + at[0], ry + at[1]];
  });
}

/** Global label text position (`GetSchematicTextOffset`) and justify, so painter.ts never has to re-derive which side of the flag the text sits on. */
export function globalLabelTextPlacement(shape: LabelShape, spin: LabelSpin, at: [number, number], sizeUm: number = LABEL_TEXT_SIZE_UM): { pos: [number, number]; justify: "left" | "center" | "right" } {
  const H = sizeUm;
  const E = kiRound(0.375 * H);
  let horiz = E;
  if (shape === "input" || shape === "bidirectional" || shape === "tri_state") horiz += kiRound((H * 3) / 4);
  const vert = Math.trunc(H * 0.0715); // truncated, not rounded -- source uses a plain (int) cast
  const [ox, oy] = spin === "left" ? [-horiz, vert] : spin === "up" ? [vert, -horiz] : spin === "right" ? [horiz, vert] : [vert, horiz];
  // Horizontal spins read left-to-right starting just past the flag's
  // point; vertical spins (up/bottom) are drawn by painter.ts with an
  // explicit text rotation, where "left" justify in the text's own
  // rotated frame produces the same "starts near the anchor, grows
  // outward" layout.
  return { pos: [at[0] + ox, at[1] + oy], justify: spin === "right" || spin === "bottom" ? "left" : spin === "left" || spin === "up" ? "right" : "center" };
}

/** Fixed-size hierarchical-label arrow templates, in units of hs = floor(H/2) -- sch_label.cpp:62-96, read directly rather than derived (these are small enough that re-deriving them from a general formula would be more error-prone than transcribing the real table). Each already includes the closing point. bidirectional and tri_state share one table (confirmed identical in source). */
const HIER_SHAPES: Record<LabelShape, Record<LabelSpin, [number, number][]>> = {
  input: {
    left: [
      [0, 0],
      [-1, -1],
      [-2, -1],
      [-2, 1],
      [-1, 1],
      [0, 0],
    ],
    up: [
      [0, 0],
      [1, -1],
      [1, -2],
      [-1, -2],
      [-1, -1],
      [0, 0],
    ],
    right: [
      [0, 0],
      [1, 1],
      [2, 1],
      [2, -1],
      [1, -1],
      [0, 0],
    ],
    bottom: [
      [0, 0],
      [1, 1],
      [1, 2],
      [-1, 2],
      [-1, 1],
      [0, 0],
    ],
  },
  output: {
    left: [
      [-2, 0],
      [-1, 1],
      [0, 1],
      [0, -1],
      [-1, -1],
      [-2, 0],
    ],
    up: [
      [0, -2],
      [1, -1],
      [1, 0],
      [-1, 0],
      [-1, -1],
      [0, -2],
    ],
    right: [
      [2, 0],
      [1, -1],
      [0, -1],
      [0, 1],
      [1, 1],
      [2, 0],
    ],
    bottom: [
      [0, 2],
      [1, 1],
      [1, 0],
      [-1, 0],
      [-1, 1],
      [0, 2],
    ],
  },
  bidirectional: {
    left: [
      [0, 0],
      [-1, -1],
      [-2, 0],
      [-1, 1],
      [0, 0],
    ],
    up: [
      [0, 0],
      [-1, -1],
      [0, -2],
      [1, -1],
      [0, 0],
    ],
    right: [
      [0, 0],
      [1, -1],
      [2, 0],
      [1, 1],
      [0, 0],
    ],
    bottom: [
      [0, 0],
      [-1, 1],
      [0, 2],
      [1, 1],
      [0, 0],
    ],
  },
  tri_state: {
    left: [
      [0, 0],
      [-1, -1],
      [-2, 0],
      [-1, 1],
      [0, 0],
    ],
    up: [
      [0, 0],
      [-1, -1],
      [0, -2],
      [1, -1],
      [0, 0],
    ],
    right: [
      [0, 0],
      [1, -1],
      [2, 0],
      [1, 1],
      [0, 0],
    ],
    bottom: [
      [0, 0],
      [-1, 1],
      [0, 2],
      [1, 1],
      [0, 0],
    ],
  },
  passive: {
    left: [
      [0, -1],
      [-2, -1],
      [-2, 1],
      [0, 1],
      [0, -1],
    ],
    up: [
      [1, 0],
      [1, -2],
      [-1, -2],
      [-1, 0],
      [1, 0],
    ],
    right: [
      [0, -1],
      [2, -1],
      [2, 1],
      [0, 1],
      [0, -1],
    ],
    bottom: [
      [1, 0],
      [1, 2],
      [-1, 2],
      [-1, 0],
      [1, 0],
    ],
  },
};

/** The small, fixed-size hierarchical-label arrow -- does not depend on the label's text at all (unlike a global label's flag). */
export function hierLabelOutline(shape: LabelShape, spin: LabelSpin, at: [number, number], sizeUm: number = LABEL_TEXT_SIZE_UM): [number, number][] {
  const hs = Math.floor(sizeUm / 2);
  return HIER_SHAPES[shape][spin].map(([x, y]) => [x * hs + at[0], y * hs + at[1]]);
}

/** Hierarchical label text position/justify -- `dist = T + (rendered text width)`, the text width itself standing in for the arrow's own reach (sch_label.cpp:2509-2526, ported). */
export function hierLabelTextPlacement(text: string, spin: LabelSpin, at: [number, number], sizeUm: number = LABEL_TEXT_SIZE_UM): { pos: [number, number]; justify: "left" | "center" | "right" } {
  const T = kiRound(0.15 * sizeUm);
  const dist = T + measureStrokeText(text, sizeUm);
  const [ox, oy] = spin === "left" ? [-dist, 0] : spin === "up" ? [0, -dist] : spin === "right" ? [dist, 0] : [0, dist];
  return { pos: [at[0] + ox, at[1] + oy], justify: spin === "right" || spin === "bottom" ? "left" : spin === "left" || spin === "up" ? "right" : "center" };
}

/** A local label has no outline at all -- just text, offset off the wire the same way a global/hierarchical label's text is (sch_label.cpp's base `CreateGraphicShape` returns nothing; offset formula at sch_label.cpp:448-465, ported). V-justify is "bottom" (text sits just above a horizontal wire) in every real local label, which painter.ts applies via its own CAP_HEIGHT baseline approximation rather than this module (stroke text has no native top/bottom baseline the way outline fonts do -- see painter.ts's existing MIDDLE_OFFSET_FACTOR/CAP_HEIGHT constants). */
export function localLabelTextPlacement(spin: LabelSpin, at: [number, number], sizeUm: number = LABEL_TEXT_SIZE_UM): { pos: [number, number]; justify: "left" | "center" | "right" } {
  const pen = autoPenWidth(sizeUm);
  const dist = kiRound(0.15 * sizeUm) + pen;
  const horizontal = spin === "left" || spin === "right";
  const [ox, oy] = horizontal ? [0, -dist] : [-dist, 0];
  return { pos: [at[0] + ox, at[1] + oy], justify: spin === "right" ? "left" : spin === "left" ? "right" : "center" };
}
