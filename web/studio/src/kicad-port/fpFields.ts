// A footprint's fields, attributes and pads as the board edits them: the ids of fields, the layouts the verbs take, the angle conversions between
// the board and the footprint's own frame, and the checks the Footprint Properties and Pad Properties dialogs make before they send anything.
//
// Ported from pcbnew/dialogs/dialog_footprint_properties.cpp (`Validate`, `OnAddField`), pcbnew/pcb_fields_grid_table.cpp (the grid's columns),
// pcbnew/pcb_field.cpp (`PCB_FIELD`, a `PCB_TEXT` with a name) and pcbnew/pcb_text.cpp (`GetDrawRotation`'s keep-upright rule) at KiCad 8303b2ad; the
// model side is crates/model/src/fp_edit.rs and the verbs crates/ops/src/fp_edit.rs.
//
// A field is an item of the board with an id of its own, `REF:Reference`, `REF:Value` or `REF:<name>` (a reference holds no colon), and so is
// selected, moved, turned and flipped by the same verbs as any other item.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, BoardText, Cmd, FieldInfo, FieldLayoutCmd, FootprintAttrsCmd, Pad, PadEditCmd, Part, Um, UserFieldCmd } from "../api/types";

export const REFERENCE = "Reference";
export const VALUE = "Value";

/** `TEXT_MIN_SIZE_MM` and `TEXT_MAX_SIZE_MM` (`include/eda_text.h`), in um. */
export const TEXT_MIN_UM = 1;
export const TEXT_MAX_UM = 250_000;

// ------------------------------------------------------------------------------------------------------------------------------- ids

/** The id of a footprint's field: `REF:Reference`, `REF:Value`, `REF:<user field name>`. */
export const fieldId = (ref: string, name: string): string => `${ref}:${name}`;

/** The footprint and the field name of a field id (the first colon splits them), or null for any other id. */
export function parseFieldId(id: string): { ref: string; name: string } | null {
  const i = id.indexOf(":");
  if (i <= 0 || i === id.length - 1) return null;
  return { ref: id.slice(0, i), name: id.slice(i + 1) };
}

/** The footprint's fields as the board shows them: Reference, Value, then the user fields. */
export const fieldsOf = (part: Part): FieldInfo[] => part.fields ?? [];

/** The field an id names and the footprint it is on, or null (the footprint is not placed, or has no such field). */
export function fieldById(board: BoardState, id: string): { part: Part; field: FieldInfo } | null {
  const parsed = parseFieldId(id);
  if (!parsed) return null;
  const part = board.parts.find((p) => p.placed && p.ref === parsed.ref);
  const field = part?.fields?.find((f) => f.id === id);
  return part && field ? { part, field } : null;
}

// --------------------------------------------------------------------------------------------------------------------------- layouts

/** The layout the verbs take for a field as the state shows it (`FieldLayout` in the footprint's own frame). */
export function layoutOf(f: FieldInfo): FieldLayoutCmd {
  return {
    at: { x: f.lx, y: f.ly },
    angle: f.langle,
    size: [f.w, f.h],
    thickness: f.thickness,
    layer: f.layer,
    visible: f.visible,
    halign: f.halign,
    valign: f.valign,
    mirror: f.mirror,
    bold: f.bold,
    italic: f.italic,
    keep_upright: f.upright,
    knockout: f.knockout,
  };
}

/** `layoutOf` with `patch` laid over it. */
export const layoutWith = (f: FieldInfo, patch: Partial<FieldLayoutCmd>): FieldLayoutCmd => ({ ...layoutOf(f), ...patch });

/** The sense of angles in a footprint's frame: a bottom-side footprint is the mirror image of the top-side one, and so are its angles. */
const sense = (part: Pick<Part, "side">): 1 | -1 => (part.side === "bottom" ? -1 : 1);

const mod360 = (a: number): number => ((a % 360_000) + 360_000) % 360_000;

/**
 * The angle in the footprint's own frame (`langle`) that makes a field's text read at `abs` on the board (millidegrees, KiCad's counter-clockwise ones):
 * the board angle is `sense * langle - rot`, `rot` being the footprint's own clockwise turn ([crates/model/src/fp_edit.rs] `FieldLayout::board_angle`).
 */
export function localAngleOf(part: Pick<Part, "side" | "rot">, abs: number): number {
  return mod360(sense(part) * (Math.round(abs) + Math.round((part.rot ?? 0) * 1000)));
}

/** The user fields as the verb takes them, in order. */
export const userFieldsOf = (part: Part): UserFieldCmd[] => fieldsOf(part).filter((f) => f.name !== REFERENCE && f.name !== VALUE).map((f) => ({ name: f.name, text: f.text, layout: layoutOf(f) }));

/** The attributes as the verb takes them. */
export function attrsOf(part: Part): FootprintAttrsCmd {
  const a = part.attrs;
  return { kind: a?.kind ?? "unspecified", board_only: a?.board_only ?? false, exclude_from_pos_files: a?.exclude_from_pos_files ?? false, exclude_from_bom: a?.exclude_from_bom ?? false, dnp: a?.dnp ?? false, allow_missing_courtyard: a?.allow_missing_courtyard ?? false };
}

/** One field's layout changed (and, for a user field, its text): `edit_board_field`, which leaves every other field of the footprint alone. */
export function editFieldCmd(part: Part, f: FieldInfo, patch: Partial<FieldLayoutCmd>, text?: string): Cmd {
  return { op: "edit_board_field", part: part.ref, name: f.name, layout: layoutWith(f, patch), ...(text !== undefined ? { text } : {}) };
}

/** The new attributes of a footprint (`attrsOf` with `patch` laid over it): `edit_board_footprint`. */
export function editAttrsCmd(part: Part, patch: Partial<FootprintAttrsCmd>): Cmd {
  return { op: "edit_board_footprint", part: part.ref, attrs: { ...attrsOf(part), ...patch } };
}

// ------------------------------------------------------------------------------------------------------------------------ drawing

/** `PCB_TEXT::GetDrawRotation`: a footprint's text that keeps upright is drawn at an angle in ]-90, 90] degrees, otherwise as it is. Millidegrees, counter-clockwise. */
export function drawAngle(angleMdeg: number, upright: boolean): number {
  let a = ((angleMdeg % 360_000) + 360_000) % 360_000;
  if (upright) {
    // ]-90..90]: bring 270 and over down by a half turn first
    if (a > 180_000) a -= 360_000;
    while (a > 90_000) a -= 180_000;
    while (a <= -90_000) a += 180_000;
  }
  return a;
}

/** A field as the free-standing board text the canvas already draws, boxes and hit-tests (`BoardText`): angle counter-clockwise, size the glyph height. */
export function fieldAsText(f: FieldInfo): BoardText {
  return {
    id: f.id,
    content: f.text,
    x: f.x,
    y: f.y,
    angle: drawAngle(f.angle, f.upright),
    layer: f.layer,
    size: f.h,
    stroke_width: f.thickness,
    justify: f.halign < 0 ? "left" : f.halign > 0 ? "right" : "center",
    mirror: f.mirror,
  };
}

// ---------------------------------------------------------------------------------------------------------------------------- pads

/** Which of the pads that share a number this is (1 for the first): the studio's `REF.NUM#k`. */
export function nthOfPad(part: Part, pad: Pad): number {
  const same = (part.pads ?? []).filter((p) => p.num === pad.num);
  return same.indexOf(pad) + 1 || 1;
}

/** The edit of a pad as it stands, to send back with the change a dialog or the panel makes: the stored overrides, or none. */
export function padEditOf(part: Part, pad: Pad): PadEditCmd {
  return { ...(pad.edit ?? {}), number: pad.num, nth: nthOfPad(part, pad) };
}

/** `padEditOf` with `patch` laid over it; a key set to `undefined` is taken out (the override goes, the library's value is back). */
export function padEditWith(part: Part, pad: Pad, patch: Partial<PadEditCmd>): PadEditCmd {
  const next: Record<string, unknown> = { ...padEditOf(part, pad), ...patch };
  for (const k of Object.keys(next)) if (next[k] === undefined) delete next[k];
  // A hole is a round one or an oblong one: giving one takes the other away.
  if (patch.drill !== undefined) delete next.drill_slot;
  if (patch.drill_slot !== undefined) delete next.drill;
  return next as unknown as PadEditCmd;
}

/** `edit_board_pad` for one pad with `patch` laid over its edit. */
export const editPadCmd = (part: Part, pad: Pad, patch: Partial<PadEditCmd>): Cmd => ({ op: "edit_board_pad", part: part.ref, edit: padEditWith(part, pad, patch) });

// ------------------------------------------------------------------------------------------------------------------------- checks

/** The messages `DIALOG_FOOTPRINT_PROPERTIES::Validate` shows for one row of the field grid, or null when the row is fine. */
export function checkFieldRow(name: string, layout: Pick<FieldLayoutCmd, "size" | "thickness">, fmt: (um: Um) => string): string | null {
  if (!name.trim()) return "Fields must have a name.";
  const [w, h] = layout.size;
  if (w < TEXT_MIN_UM) return `Text width must be at least ${fmt(TEXT_MIN_UM)}.`;
  if (w > TEXT_MAX_UM) return `Text width must be at most ${fmt(TEXT_MAX_UM)}.`;
  if (h < TEXT_MIN_UM) return `Text height must be at least ${fmt(TEXT_MIN_UM)}.`;
  if (h > TEXT_MAX_UM) return `Text height must be at most ${fmt(TEXT_MAX_UM)}.`;
  // `ClampTextPenSize`: no pen thicker than a quarter of the smaller side.
  const max = Math.round(Math.min(Math.abs(w), Math.abs(h)) * 0.25);
  if ((layout.thickness ?? 0) > max) return `Text thickness is too large for the text size.\nIt will be clamped at ${fmt(max)}.`;
  return null;
}

/** `GetUserFieldName( ordinal )`: the name a field starts with when it is added (`Field4`, `Field5` ...). */
export function newFieldName(existing: readonly string[], ordinal: number): string {
  let n = ordinal;
  for (;;) {
    const name = `Field${n}`;
    if (!existing.includes(name)) return name;
    n++;
  }
}

/** The layout `OnAddField` gives a new field: hidden, at the footprint's origin, on the fabrication layer of its side. */
export function newFieldLayout(part: Part): FieldLayoutCmd {
  const bottom = part.side === "bottom";
  return { at: { x: 0, y: 0 }, angle: localAngleOf(part, 0), size: [1000, 1000], thickness: 150, layer: bottom ? "B.Fab" : "F.Fab", visible: false, halign: 0, valign: 0, mirror: bottom, bold: false, italic: false, keep_upright: true, knockout: false };
}

/**
 * `DIALOG_PAD_PROPERTIES::padValuesOK` for the values the board's pad dialog edits: messages in the dialog's words, in its order, or null when the pad can be
 * made. `pad` is what the dialog holds.
 */
export function checkPadValues(pad: { kind: string; shape: string; size: [Um, Um]; drill?: Um | null; slot?: [Um, Um] | null; ratio?: number | null; pasteRatio?: number | null }, fmt: (um: Um) => string): string | null {
  if (pad.size[0] <= 0 || (pad.shape !== "circle" && pad.size[1] <= 0)) return "Pad size must be greater than zero.";
  const hole = pad.kind !== "smd";
  if (hole) {
    const round = pad.slot == null;
    if (round && (pad.drill ?? 0) <= 0) return "Hole size must be greater than zero.";
    if (!round && ((pad.slot?.[0] ?? 0) <= 0 || (pad.slot?.[1] ?? 0) <= 0)) return "Hole size must be greater than zero.";
    const [hw, hh] = round ? [pad.drill ?? 0, pad.drill ?? 0] : (pad.slot as [Um, Um]);
    const [pw, ph] = pad.shape === "circle" ? [pad.size[0], pad.size[0]] : pad.size;
    // A plated hole needs copper around it; a non-plated one may be as big as its pad.
    if (pad.kind === "through_hole" && (hw >= pw || hh >= ph)) return `Hole is too large for the pad: leave a ring of copper around the ${fmt(Math.max(hw, hh))} hole.`;
  }
  if (pad.shape === "round_rect" && pad.ratio != null && (pad.ratio < 0 || pad.ratio > 0.5)) return "Corner radius ratio must be between 0 and 50%.";
  if (pad.pasteRatio != null && (pad.pasteRatio < -0.5 || pad.pasteRatio > 1)) return "Solder paste ratio must be between -50% and 100%.";
  return null;
}
