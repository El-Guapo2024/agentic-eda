// The shape half of pcbnew/dialogs/dialog_pad_properties.cpp: the nine shape choices (`CHOICE_SHAPE_*`), what choosing one sets up
// (`onPadShapeSelection`), the limits of the rounded and chamfered corners (`getMaxChamferRatio`, `getMaxCornerRadius`,
// `updateAllowedPadChamferCorners`), a trapezoid's axis and delta (`transferDataToPad`), the zone knockout of a custom pad, and the basic
// shapes of a custom pad (the list the older dialogs kept: add, edit and delete a line, an arc, a circle, a rectangle, a polygon or a
// curve, as `dialog_shape_properties.cpp` words their fields). Pure -- no React -- so the dialogs stay thin views of it and the rules
// are tested.
import type { ChamferCorners, CmdShape, LibraryPad, LibraryPadShape, PadCustom, PadEditCmd, PadPrimitive, PadShapeName, PointXY, Um } from "../api/types";

/** The shape selector's nine choices, in the dialog's order: `m_PadShapeSelectorChoices`. */
export type PadShapeChoice = "circle" | "oval" | "rect" | "trapezoid" | "round_rect" | "chamfered_rect" | "chamfered_round_rect" | "custom_circle" | "custom_rect";

export const PAD_SHAPE_CHOICES: { value: PadShapeChoice; label: string }[] = [
  { value: "circle", label: "Circular" },
  { value: "oval", label: "Oval" },
  { value: "rect", label: "Rectangular" },
  { value: "trapezoid", label: "Trapezoidal" },
  { value: "round_rect", label: "Rounded rectangle" },
  { value: "chamfered_rect", label: "Chamfered rectangle" },
  { value: "chamfered_round_rect", label: "Chamfered with other corners rounded" },
  { value: "custom_circle", label: "Custom (circular base)" },
  { value: "custom_rect", label: "Custom (rectangular base)" },
];

/** `PAD_SHAPE` a choice stands for (`code_shape[]`): the two chamfered choices are one shape, the two custom ones too. */
export function shapeNameOf(choice: PadShapeChoice): PadShapeName {
  switch (choice) {
    case "circle":
      return "circle";
    case "oval":
      return "oval";
    case "rect":
      return "rect";
    case "trapezoid":
      return "trapezoid";
    case "round_rect":
      return "round_rect";
    case "chamfered_rect":
    case "chamfered_round_rect":
      return "chamfered_rect";
    case "custom_circle":
    case "custom_rect":
      return "custom";
  }
}

export const NO_CORNERS: ChamferCorners = { top_left: false, top_right: false, bottom_left: false, bottom_right: false };

/** The default corner ratio of KiCad's `PAD_UTILS::GetDefaultIpcRoundingRatio`: 25%, the radius capped at 0.25 mm (the IPC norm). */
export function defaultIpcRoundingRatio(size: [Um, Um]): number {
  const shorter = Math.min(size[0], size[1]);
  if (shorter <= 0) return 0.25;
  return Math.min(0.25, 250 / shorter);
}

/** What the dialog keeps of a pad's shape while it is open. */
export interface PadShapeForm {
  choice: PadShapeChoice;
  /** The size's x, which is the diameter of a circle and of a circular base. */
  w: Um;
  h: Um;
  /** "Corner size" of a rounded rectangle and of the chamfered one with rounded corners, `0..0.5`. */
  ratio: number;
  /** "Chamfer size", `0..1` of the shorter side. */
  chamferRatio: number;
  corners: ChamferCorners;
  /** "Trapezoid axis": Horizontal is `delta.x`, Vertical `delta.y`. */
  trapAxis: "horizontal" | "vertical";
  trapDelta: Um;
  /** "Zone knockout" of a custom pad: its shape or its convex hull. */
  knockout: PadCustom["zone"];
  primitives: PadPrimitive[];
}

/** The part of a pad the shape form reads (`Pad` of the studio state). */
export interface PadShapeSource {
  shape?: PadShapeName;
  size?: [Um, Um];
  w: Um;
  h: Um;
  round?: boolean;
  ratio?: number | null;
  delta?: [Um, Um] | null;
  chamfer_ratio?: number | null;
  chamfer_corners?: ChamferCorners | null;
  custom?: PadCustom | null;
}

/** The shape name a pad has: its own, or what an older backend's `round` and extents say. */
export function padShapeName(pad: Pick<PadShapeSource, "shape" | "round" | "w" | "h">): PadShapeName {
  return pad.shape ?? (pad.round ? (Math.abs(pad.w - pad.h) < 1 ? "circle" : "oval") : "rect");
}

/** The choice that shows a pad's shape (`initPadstackLayerValues`): a chamfered rectangle with a corner ratio is the mixed one, a custom pad has its base. */
export function choiceOf(pad: Pick<PadShapeSource, "shape" | "round" | "w" | "h" | "ratio" | "custom">): PadShapeChoice {
  switch (padShapeName(pad)) {
    case "circle":
      return "circle";
    case "oval":
      return "oval";
    case "trapezoid":
      return "trapezoid";
    case "round_rect":
      return "round_rect";
    case "chamfered_rect":
      return (pad.ratio ?? 0) > 0 ? "chamfered_round_rect" : "chamfered_rect";
    case "custom":
      return pad.custom?.anchor === "rect" ? "custom_rect" : "custom_circle";
    default:
      return "rect";
  }
}

/** The form of a pad as the dialog opens on it. */
export function shapeFormOf(pad: PadShapeSource): PadShapeForm {
  const size = pad.size ?? [pad.w, pad.h];
  const delta = pad.delta ?? [0, 0];
  return {
    choice: choiceOf(pad),
    w: size[0],
    h: size[1],
    ratio: pad.ratio ?? 0.25,
    chamferRatio: pad.chamfer_ratio ?? 0.2,
    corners: { ...(pad.chamfer_corners ?? NO_CORNERS) },
    // `initPadstackLayerValues`: the horizontal axis when delta.x is set, else the vertical one.
    trapAxis: delta[0] !== 0 || delta[1] === 0 ? "horizontal" : "vertical",
    trapDelta: delta[0] !== 0 ? delta[0] : delta[1],
    knockout: pad.custom?.zone ?? "outline",
    primitives: (pad.custom?.primitives ?? []).map((p) => ({ ...p, shape: structuredClone(p.shape) })),
  };
}

/** `m_sizeY.Enable`: a circle and a circular base have one size. */
export function hasSizeY(choice: PadShapeChoice): boolean {
  return choice !== "circle" && choice !== "custom_circle";
}

export function isChamfered(choice: PadShapeChoice): boolean {
  return choice === "chamfered_rect" || choice === "chamfered_round_rect";
}

export function isCustom(choice: PadShapeChoice): boolean {
  return choice === "custom_circle" || choice === "custom_rect";
}

/** The size the pad is given (`SetSize`): the diameter twice for a circle and a circular base. */
export function sizeOf(form: PadShapeForm): [Um, Um] {
  return hasSizeY(form.choice) ? [form.w, form.h] : [form.w, form.w];
}

/**
 * `onPadShapeSelection`: choosing a shape sets up what it needs -- a chamfered rectangle gets KiCad's 20% chamfer and one corner cut when it has
 * none (top left), the one with rounded corners the IPC corner ratio too -- and the spoke angle follows a circle (45 degrees) and leaves it (90).
 * Returns the form and the spoke angle the dialog's field should show, when it changes (`null`: left as it is).
 */
export function chooseShape(form: PadShapeForm, choice: PadShapeChoice, spokeAngleDeg: number): { form: PadShapeForm; spokeAngleDeg: number } {
  const next: PadShapeForm = { ...form, choice, corners: { ...form.corners } };
  if (choice === "round_rect" && form.ratio === 0) next.ratio = defaultIpcRoundingRatio(sizeOf(next));
  if (choice === "chamfered_rect") {
    if (form.chamferRatio === 0) next.chamferRatio = 0.2;
    // "A reasonable default is one corner chamfered (usual for some SMD pads)."
    if (!anyCorner(form.corners)) next.corners = { ...NO_CORNERS, top_left: true };
  }
  if (choice === "chamfered_round_rect" && form.ratio === 0 && form.chamferRatio === 0) {
    next.ratio = defaultIpcRoundingRatio(sizeOf(next));
    next.chamferRatio = 0.2;
  }
  // "must do this before enabling/disabling m_sizeY as we're using that as a flag to see what the last shape was"
  let angle = spokeAngleDeg;
  if (choice === "circle") {
    if (hasSizeY(form.choice) && spokeAngleDeg === 90) angle = 45;
  } else if (!hasSizeY(form.choice) && spokeAngleDeg === 45) {
    angle = 90;
  }
  return { form: next, spokeAngleDeg: angle };
}

export function anyCorner(c: ChamferCorners): boolean {
  return c.top_left || c.top_right || c.bottom_left || c.bottom_right;
}

const CORNER_ORDER = ["top_left", "top_right", "bottom_right", "bottom_left"] as const;

/** `getMaxChamferRatio`: what is left of the side after the rounded corners (`1 - corner ratio`), and half of it when two neighbouring corners are cut. */
export function maxChamferRatio(form: PadShapeForm): number {
  let baseline = form.choice === "chamfered_round_rect" ? 1 - form.ratio : 1;
  for (let i = 0; i < 4; i++) {
    if (!form.corners[CORNER_ORDER[i]!]) continue;
    if (form.corners[CORNER_ORDER[(i + 1) % 4]!] || form.corners[CORNER_ORDER[(i + 3) % 4]!]) {
      // "If two adjacent corners are selected, the maximum chamfer ratio is 50%" (the C++ takes the larger of that and the baseline).
      baseline = Math.max(baseline, 0.5);
      break;
    }
  }
  return baseline;
}

/** `getMaxCornerRadius`: the side less the chamfer. */
export function maxCornerRatio(form: PadShapeForm): number {
  return form.choice === "round_rect" ? 0.5 : 1 - form.chamferRatio;
}

/**
 * `updateAllowedPadChamferCorners`: once the chamfer is more than half the side, a corner next to a cut one cannot be cut. The checkboxes are visited in order
 * (top left, top right, bottom right, bottom left) with the state they have by then: a corner whose neighbour is cut is disabled and cleared, which can free its
 * own neighbour. Returns the corners that are left and which of them the dialog leaves enabled.
 */
export function allowedCorners(form: PadShapeForm): { corners: ChamferCorners; enabled: Record<(typeof CORNER_ORDER)[number], boolean> } {
  const corners = { ...form.corners };
  const enabled = { top_left: true, top_right: true, bottom_right: true, bottom_left: true };
  if (form.chamferRatio > 0.5) {
    for (let i = 0; i < 4; i++) {
      const disable = corners[CORNER_ORDER[(i + 1) % 4]!] || corners[CORNER_ORDER[(i + 3) % 4]!];
      enabled[CORNER_ORDER[i]!] = !disable;
      if (disable) corners[CORNER_ORDER[i]!] = false;
    }
  }
  return { corners, enabled };
}

/** Which corner checkboxes the dialog leaves enabled (`allowedCorners`). */
export function enabledCorners(form: PadShapeForm): Record<(typeof CORNER_ORDER)[number], boolean> {
  return allowedCorners(form).enabled;
}

/** The chamfer size the dialog accepts, clamped to `0..maxChamferRatio` as `onCornerSizePercentChange` does; the corners that can no longer be cut are cleared. */
export function setChamferRatio(form: PadShapeForm, ratio: number): PadShapeForm {
  const next: PadShapeForm = { ...form, corners: { ...form.corners } };
  next.chamferRatio = Math.min(Math.max(ratio, 0), maxChamferRatio(next));
  next.corners = allowedCorners(next).corners;
  return next;
}

/** The corner size (the rounded corners' ratio): `0` to `getMaxCornerRadius`, 50% for a plain rounded rectangle. */
export function setCornerRatio(form: PadShapeForm, ratio: number): PadShapeForm {
  return { ...form, ratio: Math.min(Math.max(ratio, 0), maxCornerRatio(form)) };
}

/** The deltas of the trapezoid (`transferDataToPad`): one axis only, and never past the pad's own size -- a delta that is too large is cut to the size less 2 um (and `clamped`). */
export function trapezoidDelta(form: PadShapeForm): { delta: [Um, Um]; clamped: boolean } {
  const [w, h] = sizeOf(form);
  let [dx, dy] = form.trapAxis === "horizontal" ? [form.trapDelta, 0] : [0, form.trapDelta];
  let clamped = false;
  if (dx < 0 && dx < -h) [dx, clamped] = [-h + 2, true];
  if (dx > 0 && dx > h) [dx, clamped] = [h - 2, true];
  if (dy < 0 && dy < -w) [dy, clamped] = [-w + 2, true];
  if (dy > 0 && dy > w) [dy, clamped] = [w - 2, true];
  return { delta: [dx, dy], clamped };
}

// ------------------------------------------------------------------------------------------------------------------------ what OK sends

/** The pad as the dialog would leave it, for the keys of a `PadEdit` that carry the shape. */
export function shapeEdit(form: PadShapeForm): Pick<PadEditCmd, "shape" | "size" | "roundrect_ratio" | "trapezoid_delta" | "chamfer_ratio" | "chamfer_corners" | "custom"> {
  const shape = shapeNameOf(form.choice);
  const out: ReturnType<typeof shapeEdit> = { shape, size: sizeOf(form) };
  if (form.choice === "round_rect" || form.choice === "chamfered_round_rect") out.roundrect_ratio = form.ratio;
  // `transferDataToPad`: the chamfered rectangle without rounded corners has none (`SetRoundRectRadiusRatio( 0 )`).
  if (form.choice === "chamfered_rect") out.roundrect_ratio = 0;
  if (shape === "trapezoid") out.trapezoid_delta = trapezoidDelta(form).delta;
  if (shape === "chamfered_rect") {
    out.chamfer_ratio = form.chamferRatio;
    out.chamfer_corners = { ...form.corners };
  }
  if (shape === "custom") out.custom = { anchor: form.choice === "custom_rect" ? "rect" : "circle", zone: form.knockout, primitives: form.primitives };
  return out;
}

/**
 * What differs between the pad as it is and the form, as the keys of a `PadEdit` (a key that did not change is left out, so the edit stays what the user changed).
 * `was` is the pad as the studio state has it.
 */
export function shapePatch(form: PadShapeForm, was: PadShapeSource): Partial<PadEditCmd> {
  const want = shapeEdit(form);
  const before = shapeEdit(shapeFormOf(was));
  const patch: Partial<PadEditCmd> = {};
  const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
  if (want.shape !== before.shape) patch.shape = want.shape;
  if (!same(want.size, before.size)) patch.size = want.size;
  // A ratio the new shape does not have is not sent; one it has is sent when it differs from what the pad had (KiCad's 25% when it had none).
  if (want.roundrect_ratio !== undefined && (want.roundrect_ratio !== before.roundrect_ratio || want.shape !== before.shape)) patch.roundrect_ratio = want.roundrect_ratio;
  if (want.trapezoid_delta !== undefined && !same(want.trapezoid_delta, before.trapezoid_delta)) patch.trapezoid_delta = want.trapezoid_delta;
  if (want.chamfer_ratio !== undefined && (want.chamfer_ratio !== before.chamfer_ratio || want.shape !== before.shape)) patch.chamfer_ratio = want.chamfer_ratio;
  if (want.chamfer_corners !== undefined && !same(want.chamfer_corners, before.chamfer_corners)) patch.chamfer_corners = want.chamfer_corners;
  if (want.custom !== undefined && !same(want.custom, before.custom)) patch.custom = want.custom;
  return patch;
}

/** `PAD::CheckPad` for the shape (`padValuesOK`), in its words, or null: the trapezoid delta and the chamfer, and a custom shape's basic shapes. */
export function checkShape(form: PadShapeForm): string | null {
  if (form.choice === "trapezoid" && trapezoidDelta(form).clamped) return "Error: (trapezoid delta is too large)";
  if (isChamfered(form.choice)) {
    if (form.chamferRatio < 0) return "Error: (negative corner chamfer is not allowed)";
    if (form.chamferRatio > maxChamferRatio(form) + 1e-9) return "Error: (corner chamfer is too large)";
  }
  if (isCustom(form.choice)) {
    for (let i = 0; i < form.primitives.length; i++) {
      const why = checkPrimitive(form.primitives[i]!);
      if (why) return `Error: Basic shape ${i + 1}: ${why}`;
    }
  }
  return null;
}

// ----------------------------------------------------------------------------------------------------------------------- the basic shapes

export type PrimitiveKind = CmdShape["kind"];

/** The shapes a custom pad is made of (`PCB_SHAPE`: a line, an arc, a circle, a rectangle, a polygon, a bezier curve), the way the shape dialogs call them. */
export const PRIMITIVE_KINDS: { value: PrimitiveKind; label: string }[] = [
  { value: "segment", label: "Line" },
  { value: "arc", label: "Arc" },
  { value: "circle", label: "Circle" },
  { value: "rect", label: "Rectangle" },
  { value: "polygon", label: "Polygon" },
  { value: "bezier", label: "Bezier" },
];

/** The width a new basic shape starts with, 0.2 mm. */
export const DEFAULT_PRIMITIVE_WIDTH: Um = 200;

const pt = (x: Um, y: Um): PointXY => ({ x, y });

/** A new basic shape, drawn over the pad (`w` and `h` its size) so that it can be seen and edited: a line across it, an arc over its right half, a circle in it, a rectangle, a triangle, a curve. */
export function newPrimitive(kind: PrimitiveKind, w: Um, h: Um): PadPrimitive {
  const a = Math.max(Math.round(Math.min(w, h) / 2), 200);
  const common = { layer: "", stroke_width: DEFAULT_PRIMITIVE_WIDTH, filled: false };
  let shape: CmdShape;
  switch (kind) {
    case "segment":
      shape = { kind, ...common, start: pt(0, 0), end: pt(2 * a, 0) };
      break;
    case "arc":
      shape = { kind, ...common, start: pt(0, -a), mid: pt(a, 0), end: pt(0, a) };
      break;
    case "circle":
      shape = { kind, ...common, filled: true, center: pt(0, 0), end: pt(a, 0) };
      break;
    case "rect":
      shape = { kind, ...common, filled: true, start: pt(-a, -a), end: pt(a, a) };
      break;
    case "polygon":
      shape = { kind, ...common, filled: true, stroke_width: 0, pts: [pt(0, -a), pt(2 * a, a), pt(0, a)] };
      break;
    case "bezier":
      shape = { kind, ...common, start: pt(0, 0), c1: pt(a, -a), c2: pt(2 * a, a), end: pt(3 * a, 0) };
      break;
  }
  return { shape };
}

export interface PrimitiveField {
  key: string;
  label: string;
  value: number;
  /** A length in micrometres or a point's coordinate, vs. a plain number. */
  kind: "length";
}

/** Whether a shape can be filled (`PCB_SHAPE`'s Fill: a circle, a rectangle and a polygon). */
export function canFill(shape: CmdShape): boolean {
  return shape.kind === "circle" || shape.kind === "rect" || shape.kind === "polygon";
}

const XY = (name: string, label: string, p: PointXY): PrimitiveField[] => [
  { key: `${name}.x`, label: `${label} X`, value: p.x, kind: "length" },
  { key: `${name}.y`, label: `${label} Y`, value: p.y, kind: "length" },
];

/** The numbers a basic shape is edited by, in `DIALOG_SHAPE_PROPERTIES`'s wording: Start, Mid, End, Center, Radius, Control points, Line width. A polygon's points are edited one by one. */
export function primitiveFields(shape: CmdShape): PrimitiveField[] {
  const width: PrimitiveField = { key: "stroke_width", label: "Line width", value: shape.stroke_width, kind: "length" };
  switch (shape.kind) {
    case "segment":
    case "rect":
      return [...XY("start", "Start", shape.start), ...XY("end", "End", shape.end), width];
    case "arc":
      return [...XY("start", "Start", shape.start), ...XY("mid", "Midpoint", shape.mid), ...XY("end", "End", shape.end), width];
    case "circle":
      return [...XY("center", "Center", shape.center), { key: "radius", label: "Radius", value: radiusOf(shape), kind: "length" }, width];
    case "bezier":
      return [...XY("start", "Start", shape.start), ...XY("c1", "Control point 1", shape.c1), ...XY("c2", "Control point 2", shape.c2), ...XY("end", "End", shape.end), width];
    case "polygon":
      return [width];
  }
}

/** A circle's radius: the distance from its center to the point its `end` names (`EDA_SHAPE::GetRadius`). */
export function radiusOf(shape: Extract<CmdShape, { kind: "circle" }>): Um {
  return Math.round(Math.hypot(shape.end.x - shape.center.x, shape.end.y - shape.center.y));
}

/** A basic shape with one of its numbers set (`key` as `primitiveFields` names it). The radius moves the circle's `end` along its x axis from the center. */
export function setPrimitiveField(shape: CmdShape, key: string, value: number): CmdShape {
  const next = structuredClone(shape) as CmdShape;
  if (key === "stroke_width") {
    next.stroke_width = Math.max(0, Math.round(value));
    return next;
  }
  if (key === "radius" && next.kind === "circle") {
    next.end = pt(next.center.x + Math.round(value), next.center.y);
    return next;
  }
  const [name, axis] = key.split(".") as [string, "x" | "y"];
  const holder = next as unknown as Record<string, PointXY>;
  if (holder[name] && (axis === "x" || axis === "y")) holder[name]![axis] = Math.round(value);
  return next;
}

/** A basic shape's fill (`PCB_SHAPE`'s Fill: None or Solid); a shape that cannot be filled keeps none. */
export function setFilled(shape: CmdShape, filled: boolean): CmdShape {
  return canFill(shape) ? { ...shape, filled } : shape;
}

/** A polygon's points with one moved, added or taken away (not below the three a polygon needs). */
export function polygonWith(shape: Extract<CmdShape, { kind: "polygon" }>, change: { set?: [number, PointXY]; add?: PointXY; remove?: number }): CmdShape {
  const pts = shape.pts.map((p) => ({ ...p }));
  if (change.set) pts[change.set[0]] = change.set[1];
  if (change.add) pts.push(change.add);
  if (change.remove !== undefined && pts.length > 3) pts.splice(change.remove, 1);
  return { ...shape, pts };
}

/** Why a basic shape cannot be drawn, or null (the same checks the verb makes on its side). */
export function checkPrimitive(prim: PadPrimitive): string | null {
  const s = prim.shape;
  if (s.stroke_width < 0) return "the line width cannot be negative";
  switch (s.kind) {
    case "polygon":
      return s.pts.length < 3 ? "a polygon needs at least three points" : null;
    case "circle":
      return s.center.x === s.end.x && s.center.y === s.end.y ? "a circle needs a radius" : null;
    case "segment":
      return s.start.x === s.end.x && s.start.y === s.end.y ? "a line needs a length" : null;
    case "arc": {
      const same = (a: PointXY, b: PointXY) => a.x === b.x && a.y === b.y;
      return same(s.start, s.end) || same(s.mid, s.start) || same(s.mid, s.end) ? "an arc needs three different points" : null;
    }
    default:
      return null;
  }
}

/** One row of the list of basic shapes: what it is and where, in `fmt`'s length. */
export function describePrimitive(prim: PadPrimitive, fmt: (um: Um) => string): string {
  const s = prim.shape;
  const p = (q: PointXY) => `(${fmt(q.x)}, ${fmt(q.y)})`;
  const label = PRIMITIVE_KINDS.find((k) => k.value === s.kind)?.label ?? s.kind;
  const proxy = prim.proxy ? " (not copper)" : "";
  switch (s.kind) {
    case "segment":
    case "rect":
      return `${label} ${p(s.start)} to ${p(s.end)}, width ${fmt(s.stroke_width)}${s.filled ? ", filled" : ""}${proxy}`;
    case "arc":
      return `${label} ${p(s.start)} through ${p(s.mid)} to ${p(s.end)}, width ${fmt(s.stroke_width)}${proxy}`;
    case "circle":
      return `${label} at ${p(s.center)}, radius ${fmt(radiusOf(s))}, width ${fmt(s.stroke_width)}${s.filled ? ", filled" : ""}${proxy}`;
    case "polygon":
      return `${label} of ${s.pts.length} points from ${p(s.pts[0] ?? pt(0, 0))}, width ${fmt(s.stroke_width)}${s.filled ? ", filled" : ""}${proxy}`;
    case "bezier":
      return `${label} ${p(s.start)} to ${p(s.end)}, width ${fmt(s.stroke_width)}${proxy}`;
  }
}

/** The list with the basic shape at `index` replaced, taken away, or one added at the end (`ReplacePrimitives`: the list is replaced whole). */
export function primitivesWith(list: PadPrimitive[], change: { replace?: [number, PadPrimitive]; remove?: number; add?: PadPrimitive; duplicate?: number }): PadPrimitive[] {
  const out = list.map((p) => ({ ...p, shape: structuredClone(p.shape) }));
  if (change.replace) out[change.replace[0]] = change.replace[1];
  if (change.add) out.push(change.add);
  if (change.duplicate !== undefined && out[change.duplicate]) out.splice(change.duplicate + 1, 0, { ...out[change.duplicate]!, shape: structuredClone(out[change.duplicate]!.shape) });
  if (change.remove !== undefined) out.splice(change.remove, 1);
  return out;
}

// ------------------------------------------------------------------------------------------------------------- the footprint editor's pad

/** The shape form of a library pad of the footprint editor (`LibraryPad`: the same fields under its own names). */
export function shapeFormOfLibraryPad(pad: Pick<LibraryPad, "shape" | "size" | "roundrect_ratio" | "trapezoid_delta" | "chamfer_ratio" | "chamfer_corners" | "custom">): PadShapeForm {
  return shapeFormOf({
    shape: pad.shape,
    size: pad.size,
    w: pad.size[0],
    h: pad.size[1],
    ratio: pad.roundrect_ratio,
    delta: pad.trapezoid_delta,
    chamfer_ratio: pad.chamfer_ratio,
    chamfer_corners: pad.chamfer_corners,
    custom: pad.custom,
  });
}

/** The fields of a library pad that the shape form sets: its shape, size, corner ratio, trapezoid delta, chamfer and custom shapes (what does not belong to the shape is null). */
export function libraryFieldsOf(form: PadShapeForm): Pick<LibraryPad, "shape" | "size" | "roundrect_ratio" | "trapezoid_delta" | "chamfer_ratio" | "chamfer_corners" | "custom"> {
  const e = shapeEdit(form);
  return {
    shape: e.shape as LibraryPadShape,
    size: e.size as [Um, Um],
    roundrect_ratio: e.roundrect_ratio ?? null,
    trapezoid_delta: e.trapezoid_delta ?? null,
    chamfer_ratio: e.chamfer_ratio ?? null,
    chamfer_corners: e.chamfer_corners ?? { ...NO_CORNERS },
    custom: e.custom ?? null,
  };
}
