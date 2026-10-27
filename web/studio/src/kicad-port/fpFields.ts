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
import type { BoardState, BoardText, Cmd, FieldInfo, FieldLayoutCmd, FootprintAttrsCmd, Pad, PadEditCmd, Part, Nm, TextGraphicsDefaults, UserFieldCmd } from "../api/types";
import { objectOn } from "./appearance";
import { isBackLayer, textStyleOf } from "./layerDefaults";

export const REFERENCE = "Reference";
export const VALUE = "Value";

/** `TEXT_MIN_SIZE_MM` and `TEXT_MAX_SIZE_MM` (`include/eda_text.h`), in um. */
export const TEXT_MIN_NM = 1000;
export const TEXT_MAX_NM = 250_000_000;

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
    // What `${NAME}` in the text stands for: the canvas draws, boxes and hit-tests the text as it reads (`PCB_FIELD::GetShownText`).
    content: f.shown ?? f.text,
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
export function checkFieldRow(name: string, layout: Pick<FieldLayoutCmd, "size" | "thickness">, fmt: (um: Nm) => string): string | null {
  if (!name.trim()) return "Fields must have a name.";
  const [w, h] = layout.size;
  if (w < TEXT_MIN_NM) return `Text width must be at least ${fmt(TEXT_MIN_NM)}.`;
  if (w > TEXT_MAX_NM) return `Text width must be at most ${fmt(TEXT_MAX_NM)}.`;
  if (h < TEXT_MIN_NM) return `Text height must be at least ${fmt(TEXT_MIN_NM)}.`;
  if (h > TEXT_MAX_NM) return `Text height must be at most ${fmt(TEXT_MAX_NM)}.`;
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

/**
 * The layout `OnAddField` gives a new field: hidden, at the footprint's origin, on the fabrication layer of its side, styled from Board Setup's Text & Graphics
 * defaults (`newField.StyleFromSettings( m_frame->GetDesignSettings(), true )`: the Fab class's size, thickness, italic and keep-upright, mirrored on a back
 * layer). `defaults` absent is KiCad's factory table.
 */
export function newFieldLayout(part: Part, defaults?: TextGraphicsDefaults): FieldLayoutCmd {
  const bottom = part.side === "bottom";
  const layer = bottom ? "B.Fab" : "F.Fab";
  const st = textStyleOf(defaults, layer);
  return { at: { x: 0, y: 0 }, angle: localAngleOf(part, 0), size: [st.widthNm, st.heightNm], thickness: st.thicknessNm, layer, visible: false, halign: 0, valign: 0, mirror: isBackLayer(layer), bold: false, italic: st.italic, keep_upright: st.upright, knockout: false };
}

/**
 * `DIALOG_PAD_PROPERTIES::padValuesOK` for the values the board's pad dialog edits: messages in the dialog's words, in its order, or null when the pad can be
 * made. `pad` is what the dialog holds.
 */
type PadValues = {
  kind: string;
  shape: string;
  size: [Nm, Nm];
  drill?: Nm | null;
  slot?: [Nm, Nm] | null;
  ratio?: number | null;
  pasteRatio?: number | null;
  /** The shape's offset from the pad's position (where the hole is). */
  offset?: [Nm, Nm] | null;
  /** The local clearance, solder mask margin and solder paste margin the dialog sets (null: none). */
  clearance?: Nm | null;
  maskMargin?: Nm | null;
  pasteMargin?: Nm | null;
};

/** `EDA_UNIT_UTILS::UI::StringFromValue( .., aAddUnitsText = true )` of a length in nm: `%.10f` in mm, `%.5f` in mils, `%.8f` in inches, trailing zeros removed. */
export function stringFromValue(nm: Nm, units: "mm" | "mil" | "in"): string {
  const [v, digits, suffix] = units === "mil" ? [nm / 25_400, 5, " mils"] : units === "in" ? [nm / 25_400_000, 8, " in"] : [nm / 1_000_000, 10, " mm"];
  const trim = (t: string) => (t.includes(".") ? t.replace(/0+$/, "").replace(/\.$/, "") : t);
  let text = trim(v.toFixed(digits));
  if (v !== 0 && (text === "0" || text === "-0")) text = trim(v.toFixed(10));
  return text + suffix;
}

/** Whether the point (x, y) lies in the stadium of width `w` and height `h` centred on the origin (a hole: `GetEffectiveHoleShape`). */
function inStadium(x: number, y: number, w: number, h: number): boolean {
  const r = Math.min(w, h) / 2;
  const half = Math.max(w, h) / 2 - r;
  const [a, b] = w >= h ? [x, y] : [y, x];
  const dx = Math.max(Math.abs(a) - half, 0);
  return dx * dx + b * b <= r * r + 1e-6;
}

/** Points round the outline of the stadium `w` x `h` centred on the origin (a circle when `w == h`). */
function stadiumOutline(w: number, h: number): number[][] {
  const r = Math.min(w, h) / 2;
  const half = Math.max(w, h) / 2 - r;
  const out: number[][] = [];
  for (let i = 0; i < 32; i++) {
    const a = (i / 32) * 2 * Math.PI;
    const along = Math.cos(a) * r + (Math.cos(a) >= 0 ? half : -half);
    const across = Math.sin(a) * r;
    out.push(w >= h ? [along, across] : [across, along]);
  }
  return out;
}

/**
 * `DIALOG_PAD_PROPERTIES::padValuesOK` over `PAD::doCheckPad( .., aForPadProperties = true )`: the errors (`DRCE_PADSTACK_INVALID`, `DRCE_PAD_TH_WITH_NO_HOLE`)
 * keep the dialog open; the warnings (`DRCE_PADSTACK`, "Warning: " in front) are shown and the pad is applied. "(PTH pad hole leaves no copper)" is a pad the hole
 * covers whole, "(PTH pad hole not fully inside copper)" a hole that pokes out of the pad (the hole's stadium at the pad's position against the pad's outline at
 * its shape offset; a rounded or chamfered corner is taken as square), "(pad hole not inside pad shape)" a non-plated hole whose centre is off the copper. Then
 * the clearance and the mask and paste margins. KiCad has no message of its own for the corner and the paste ratios (the dialog clamps them). `units` gives the
 * minimum drill's text as `StringFromValue` writes it (`0.000004 mm`); without it `fmt` does.
 */
export function padCheck(pad: PadValues, fmt: (nm: Nm) => string, units?: "mm" | "mil" | "in"): { errors: string[]; warnings: string[] } {
  const errors: string[] = [];
  const warnings: string[] = [];
  if (pad.size[0] <= 0 || (pad.shape !== "circle" && pad.shape !== "custom" && pad.size[1] <= 0)) errors.push("Error: (Pad must have a positive size)");
  const [pw, ph] = pad.shape === "circle" ? [pad.size[0], pad.size[0]] : pad.size;
  const [ox, oy] = pad.offset ?? [0, 0];
  // the copper's outline, about the pad's position (the hole's centre)
  const padOutline = (pad.shape === "circle" || pad.shape === "oval" ? stadiumOutline(pw, ph) : [[pw / 2, ph / 2], [pw / 2, -ph / 2], [-pw / 2, ph / 2], [-pw / 2, -ph / 2]]).map(([x, y]) => [x! + ox, y! + oy]);
  const inPad = (x: number, y: number) => (pad.shape === "circle" || pad.shape === "oval" ? inStadium(x - ox, y - oy, pw, ph) : Math.abs(x - ox) <= pw / 2 && Math.abs(y - oy) <= ph / 2);
  if (pad.kind !== "smd") {
    const round = pad.slot == null;
    const [hw, hh] = round ? [pad.drill ?? 0, pad.drill ?? 0] : (pad.slot as [Nm, Nm]);
    // `CheckPad`: `DRCE_PAD_TH_WITH_NO_HOLE` for a plated or non-plated pad without a hole
    if (hw <= 0 || hh <= 0) errors.push("Error: Through hole pad has no hole.");
    else {
      const MIN_DRILL = 4;
      if (hw <= MIN_DRILL || hh <= MIN_DRILL) errors.push(`Error: (PTH pad hole size must be larger than ${units ? stringFromValue(MIN_DRILL, units) : fmt(MIN_DRILL)})`);
      if (pad.kind === "through_hole") {
        // the pad's outline inside the hole: nothing of the copper is left
        if (padOutline.every(([x, y]) => inStadium(x!, y!, hw, hh))) warnings.push("Warning: (PTH pad hole leaves no copper)");
        else if (!stadiumOutline(hw, hh).every(([x, y]) => inPad(x!, y!))) warnings.push("Warning: (PTH pad hole not fully inside copper)");
      } else if (!inPad(0, 0)) {
        // "Test only if the pad hole's centre is inside the copper area"
        warnings.push("Warning: (pad hole not inside pad shape)");
      }
    }
  }
  if ((pad.clearance ?? 0) < 0) warnings.push("Warning: (negative local clearance values have no effect)");
  const mask = pad.maskMargin ?? null;
  if (mask != null && mask < 0 && pad.shape !== "custom" && (Math.abs(mask) > pw || Math.abs(mask) > ph)) warnings.push("Warning: (negative solder mask clearance is larger than pad; no solder mask will be generated)");
  // `paste_size = pad_size + paste_margin + KiROUND( pad_size * mratio )`
  const margin = pad.pasteMargin ?? 0;
  const ratio = pad.pasteRatio ?? 0;
  if (pw + margin + Math.round(pw * ratio) <= 0 || ph + margin + Math.round(ph * ratio) <= 0) warnings.push("Warning: (negative solder paste margin is larger than pad; no solder paste mask will be generated)");
  return { errors, warnings };
}

/** The first error [`padCheck`] finds, or null (the warnings do not stop the dialog). */
export function checkPadValues(pad: PadValues, fmt: (um: Nm) => string): string | null {
  return padCheck(pad, fmt).errors[0] ?? null;
}

/**
 * `PCB_FIELD::ViewGetLOD` and `PCB_SELECTION_TOOL::Selectable`: whether the Appearance panel's Objects rows let a footprint's field show -- the Reference needs
 * References, the Value Values, every field Footprint Text, and the footprint's side must show (Footprints Front or Back). The field's own layer is the caller's.
 * `forced` is the footprint being selected: its fields then show whatever the rows say (`m_ForceShowFieldsWhenFPSelected`, on by default) -- the painter's rule,
 * not the selection tool's.
 */
export function fieldShownByObjects(layerVisible: Readonly<Record<string, boolean>>, part: Pick<Part, "side">, field: Pick<FieldInfo, "name">, forced = false): boolean {
  if (forced) return true;
  return (
    objectOn(layerVisible, part.side === "bottom" ? "footprints_back" : "footprints_front") &&
    objectOn(layerVisible, "footprint_text") &&
    (field.name !== REFERENCE || objectOn(layerVisible, "footprint_references")) &&
    (field.name !== VALUE || objectOn(layerVisible, "footprint_values"))
  );
}

