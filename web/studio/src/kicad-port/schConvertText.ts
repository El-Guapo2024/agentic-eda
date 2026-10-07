// Change To Label / Global Label / Hierarchical Label / Directive Label / Text / Text Box --
// `SCH_EDIT_TOOL::ChangeTextType` (sch_edit_tool.cpp at 8303b2ad). Every selected label, text, text box or directive
// label that is not already of the target type is replaced by a new item of that type at the same place, carrying
// its text (a label's text unescaped, a new label's text made a valid net name), its shape and its text size.
//
// The studio's labels keep their orientation implicit (it is read off the wire they sit on) and have one fixed text
// size, so the spin and size bookkeeping KiCad does per item is, here, a text angle for the new text / box and a
// pole direction for a new directive label. The result is plain verbs -- delete the old items, add the new ones -- so
// the whole conversion is one batch and one undo step, like the single `SCH_COMMIT::Push`.
import type { Cmd, LabelScope, LabelShape } from "../api/types";
import type { SchGraphic, SchGraphicInput } from "../api/schEditTypes";
import { CAP_HEIGHT_RATIO, defaultTextBoxMargin, INTERLINE_PITCH, type Measure } from "./schTextBox";
import { graphicId, labelId, textId } from "./schIds";
import { EMPTY_LABEL_TEXT, unescapeString, validNetName } from "./schNetName";

/** Which way a label reads (`SPIN_STYLE`): `right` and `left` run horizontally, `up` and `bottom` vertically. */
export type Spin = "left" | "up" | "right" | "bottom";

export type ConvertTarget = "label" | "global_label" | "hier_label" | "directive_label" | "text" | "text_box";

export type ConvertSource =
  | { kind: "label"; id: string; net: string; at: readonly [number, number]; scope: LabelScope; shape: LabelShape | null; spin: Spin }
  | { kind: "text"; id: string; content: string; at: readonly [number, number]; angleDeg: number; sizeUm: number }
  | { kind: "text_box"; id: string; graphic: SchGraphic }
  | { kind: "directive"; id: string; graphic: SchGraphic };

/** The `Cmd`s of one conversion plus the ids the new items will get (to select them afterwards). */
export interface Conversion {
  cmds: Cmd[];
  predictedIds: string[];
}

/** `eeschema/default_values.h` DEFAULT_TEXT_SIZE (50 mil), um. */
const DEFAULT_TEXT_SIZE_UM = 1270;
/** `DEFAULT_PINNED_LENGTH`-style default pole of a directive label (100 mil), um. */
const DIRECTIVE_POLE_UM = 2540;

const sourceTarget = (s: ConvertSource): ConvertTarget =>
  s.kind === "label" ? (s.scope === "local" ? "label" : s.scope === "global" ? "global_label" : "hier_label") : s.kind === "text" ? "text" : s.kind === "text_box" ? "text_box" : "directive_label";

/** The text a source carries (`txt` in ChangeTextType): a label's unescaped, a directive label's `<empty>`. */
function sourceText(s: ConvertSource): string {
  switch (s.kind) {
    case "label":
      return unescapeString(s.net);
    case "text":
      return s.content;
    case "directive":
      return EMPTY_LABEL_TEXT;
    case "text_box":
      return s.graphic.shape.type === "text_box" ? s.graphic.shape.text : "";
  }
}

/** The label shape a conversion carries over: a label keeps its own, a text has none (`L_UNSPECIFIED`, "passive"), a local label's is its constructor default `L_INPUT`. */
function sourceShape(s: ConvertSource): LabelShape {
  if (s.kind === "label") return s.shape ?? "input";
  return "passive";
}

const spinAngle = (spin: Spin): number => (spin === "up" || spin === "bottom" ? 90_000 : 0);
const spinOrientation = (spin: Spin): number => ({ right: 0, up: 90_000, left: 180_000, bottom: 270_000 })[spin];

/** Where a source sits (`item->GetPosition()`): a text box's anchor is its first corner. */
function sourcePosition(s: ConvertSource): [number, number] {
  switch (s.kind) {
    case "label":
    case "text":
      return [s.at[0], s.at[1]];
    case "text_box":
      return s.graphic.shape.type === "text_box" ? [s.graphic.shape.start.x, s.graphic.shape.start.y] : [0, 0];
    case "directive":
      return s.graphic.shape.type === "directive" ? [s.graphic.shape.at.x, s.graphic.shape.at.y] : [0, 0];
  }
}

function sourceSpin(s: ConvertSource): Spin {
  if (s.kind === "label") return s.spin;
  if (s.kind === "directive" && s.graphic.shape.type === "directive") {
    const o = Math.round((s.graphic.shape.orientation ?? 0) / 1000) % 360;
    return o === 90 ? "up" : o === 180 ? "left" : o === 270 ? "bottom" : "right";
  }
  if (s.kind === "text_box" && s.graphic.shape.type === "text_box") {
    const vertical = Math.round((s.graphic.shape.angle ?? 0) / 1000) % 180 === 90;
    const right = s.graphic.shape.h_align === "right";
    return vertical ? (right ? "bottom" : "up") : right ? "left" : "right";
  }
  return "right";
}

/** A text box's position for a new label: the middle of its left/right (or top/bottom) edge, the way ChangeTextType places it, snapped to `grid` when given. */
function textBoxLabelPosition(s: Extract<ConvertSource, { kind: "text_box" }>, grid: ((p: [number, number]) => [number, number]) | undefined): [number, number] {
  const g = s.graphic.shape;
  if (g.type !== "text_box") return [0, 0];
  const left = Math.min(g.start.x, g.end.x);
  const right = Math.max(g.start.x, g.end.x);
  const top = Math.min(g.start.y, g.end.y);
  const bottom = Math.max(g.start.y, g.end.y);
  const spin = sourceSpin(s);
  const cx = (left + right) / 2;
  const cy = (top + bottom) / 2;
  const p: [number, number] = spin === "bottom" ? [cx, top] : spin === "up" ? [cx, bottom] : spin === "left" ? [right, cy] : [left, cy];
  return grid ? grid(p) : p;
}

/** The rectangle a text box gets around a source's text -- its extent grown by the margin, plus 1/20 of the margin so the text does not rewrap. */
function textBoxAround(s: ConvertSource, text: string, measure: Measure): SchGraphicInput {
  const size = s.kind === "text" ? s.sizeUm : DEFAULT_TEXT_SIZE_UM;
  const lines = text.split("\n");
  const along = Math.max(...lines.map((l) => measure(l, size)), size);
  const across = size * CAP_HEIGHT_RATIO + (lines.length - 1) * size * INTERLINE_PITCH;
  const [x, y] = sourcePosition(s);
  const spin = sourceSpin(s);
  const vertical = s.kind === "text" ? Math.round(s.angleDeg) % 180 === 90 : spin === "up" || spin === "bottom";
  const margin = defaultTextBoxMargin(size, 0);
  // `new_textbox->GetLegacyTextMargin() / 20`: a sliver more room in the text direction, so a change in the font does not rewrap the text.
  const slop = Math.round(margin / 20);
  // The text's own rectangle from the source's anchor: horizontal text runs right from the anchor with its glyphs above the baseline; vertical
  // text runs up from the anchor with its glyphs to the left of the baseline.
  const left = vertical ? x - size * CAP_HEIGHT_RATIO : x;
  const top = vertical ? y - along : y - size * CAP_HEIGHT_RATIO;
  const right = vertical ? left + across : x + along;
  const bottom = vertical ? y : top + across;
  return {
    shape: {
      type: "text_box",
      start: { x: Math.round(left - margin), y: Math.round(top - margin - (vertical ? slop : 0)) },
      end: { x: Math.round(right + margin + (vertical ? 0 : slop)), y: Math.round(bottom + margin) },
      text,
      angle: vertical ? 90_000 : 0,
      size_um: size,
      h_align: "left",
      v_align: "top",
    },
  };
}

/**
 * The verbs that turn `sources` into `target` items. Sources already of the target type are left alone; the old items go first and the
 * new ones are added after, so each new item gets the id its content hashes to.
 */
export function convertCmds(sources: readonly ConvertSource[], target: ConvertTarget, opts: { measure: Measure; snap?: (p: [number, number]) => [number, number]; takenIds?: ReadonlySet<string> }): Conversion {
  const deletes: Cmd[] = [];
  const adds: Cmd[] = [];
  const predictedIds: string[] = [];
  const taken = new Set(opts.takenIds ?? []);
  for (const s of sources) {
    if (sourceTarget(s) === target) continue;
    const text = sourceText(s);
    const shape = sourceShape(s);
    const spin = sourceSpin(s);
    const at: [number, number] = s.kind === "text_box" && (target === "label" || target === "global_label" || target === "hier_label") ? textBoxLabelPosition(s, opts.snap) : sourcePosition(s);

    deletes.push(s.kind === "label" ? { op: "delete_label", id: s.id } : s.kind === "text" ? { op: "delete_sch_text", id: s.id } : { op: "sch_edit", verb: "delete_graphic", id: s.id });
    taken.delete(s.id);

    switch (target) {
      case "label":
      case "global_label":
      case "hier_label": {
        const net = validNetName(text);
        adds.push({ op: "add_label", net, at: { x: at[0], y: at[1] }, kind: target === "label" ? { scope: "local" } : { scope: target === "global_label" ? "global" : "hierarchical", shape } });
        const id = labelId(net, at, taken);
        taken.add(id);
        predictedIds.push(id);
        break;
      }
      case "text": {
        const angle = s.kind === "text" ? Math.round(s.angleDeg * 1000) : s.kind === "label" ? spinAngle(spin) : 0;
        adds.push({ op: "add_sch_text", content: text, at: { x: at[0], y: at[1] }, angle_millideg: angle, size_um: s.kind === "text" ? s.sizeUm : DEFAULT_TEXT_SIZE_UM });
        const id = textId(text, at, taken);
        taken.add(id);
        predictedIds.push(id);
        break;
      }
      case "directive_label": {
        // A directive label has no text of its own: a text becomes its `Netclass` field ("If we're copying from a text object assume the text is the netclass name").
        const netclass = s.kind === "text" || s.kind === "text_box" ? text : "";
        const graphic: SchGraphicInput = { shape: { type: "directive", at: { x: at[0], y: at[1] }, orientation: spinOrientation(spin), shape: "round", pin_length_um: DIRECTIVE_POLE_UM, netclass, component_class: "" } };
        adds.push({ op: "sch_edit", verb: "add_graphic", graphic });
        const id = graphicId(graphic.shape, taken);
        taken.add(id);
        predictedIds.push(id);
        break;
      }
      case "text_box": {
        const graphic = textBoxAround(s, text, opts.measure);
        adds.push({ op: "sch_edit", verb: "add_graphic", graphic });
        const id = graphicId(graphic.shape, taken);
        taken.add(id);
        predictedIds.push(id);
        break;
      }
    }
  }
  return { cmds: [...deletes, ...adds], predictedIds };
}
