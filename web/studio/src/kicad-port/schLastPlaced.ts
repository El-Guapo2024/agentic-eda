// What the schematic drawing tool remembers of the last label and text it put down, so the next dialog opens with it: `SCH_DRAWING_TOOLS::m_lastTextOrientation`,
// `m_lastTextBold`, `m_lastTextItalic`, `m_lastGlobalLabelShape`, `m_lastAutoLabelRotateOnPlacement`, `m_lastTextHJustify`, `m_lastTextVJustify` and `m_lastTextAngle`
// (eeschema/tools/sch_drawing_tools.cpp at 8303b2ad). Per session, like the tool's: a reload starts again from the values the tool is constructed with.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LabelShape } from "../api/types";
import type { SchHAlign, SchVAlign } from "../api/schEditTypes";

export type LabelSpinName = "right" | "up" | "left" | "bottom";

export interface LastPlaced {
  /** A label's spin; "" is none chosen yet, which reads off the wire the label ends at as every label did before a spin could be picked. */
  spin: LabelSpinName | "";
  bold: boolean;
  italic: boolean;
  /** The shape of the last global or hierarchical label. */
  shape: LabelShape;
  /** The "Auto" check of the last global or hierarchical label (`m_lastAutoLabelRotateOnPlacement`, false until one was placed with it). */
  autoRotate: boolean;
  /** A free text's justification: the tool is constructed with centre and centre (`m_lastTextHJustify( GR_TEXT_H_ALIGN_CENTER )`). */
  textH: SchHAlign;
  textV: SchVAlign;
  textVertical: boolean;
}

const INITIAL: LastPlaced = { spin: "", bold: false, italic: false, shape: "input", autoRotate: false, textH: "center", textV: "center", textVertical: false };

let last: LastPlaced = { ...INITIAL };

/** What the next label or text dialog opens with. */
export function lastPlaced(): Readonly<LastPlaced> {
  return last;
}

/** Remember what the dialog that just placed an item ended with. */
export function rememberPlaced(patch: Partial<LastPlaced>): void {
  last = { ...last, ...patch };
}

/** Back to the values the tool starts with (the tests, and a fresh session). */
export function resetLastPlaced(): void {
  last = { ...INITIAL };
}
