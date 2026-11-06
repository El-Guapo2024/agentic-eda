// What a library footprint keeps of its graphics and texts by id beyond the editor's `CmdShape` / `CmdText` (`crates/model/src/lib_keep.rs` `LibraryFootprintKeep`, UI-LEFT F5):
// a graphic's line style and hatched fill, a text's bold, italic, vertical justification, glyph width, knockout and keep-upright, the locked items and the hidden texts.
// The Footprint Editor's shape and text dialogs read and set them (`fp_item set_graphic` / `set_text` with a look), the painter draws them.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { ShapeLook, TextLook } from "../api/fpEditorTypes";

/** The kept record as the server sends it (`LibraryFootprint.kicad`). */
interface Keep {
  stroke_types?: Record<string, string>;
  hatches?: Record<string, ShapeLook["hatch"]>;
  text_extras?: { id: string; width_nm?: number | null; valign?: "top" | "center" | "bottom"; bold?: boolean; italic?: boolean; knockout?: boolean; face?: string | null }[];
  unlocked_texts?: string[];
  locked?: string[];
  hidden_texts?: string[];
}

type WithKeep = { kicad?: Record<string, unknown> } | null | undefined;

const keepOf = (fp: WithKeep): Keep => (fp?.kicad ?? {}) as Keep;

/** The graphic's line style (`solid` when it keeps none) and hatch. */
export function shapeLookOf(fp: WithKeep, id: string): ShapeLook {
  const k = keepOf(fp);
  return { line_style: k.stroke_types?.[id] ?? "solid", hatch: k.hatches?.[id] ?? null };
}

/** The text's looks beyond `CmdText`; `size` is its height (`CmdText.size_nm`), the width when it keeps none of its own. */
export function textLookOf(fp: WithKeep, id: string): TextLook {
  const k = keepOf(fp);
  const e = k.text_extras?.find((x) => x.id === id);
  return {
    bold: !!e?.bold,
    italic: !!e?.italic,
    valign: e?.valign === "top" ? -1 : e?.valign === "bottom" ? 1 : 0,
    width_nm: e?.width_nm ?? null,
    knockout: !!e?.knockout,
    keep_upright: !(k.unlocked_texts ?? []).includes(id),
  };
}

/** `PCB_TEXT::IsVisible`: a text of the footprint read hidden (`(hide yes)`) is not drawn and not picked (`ViewGetLOD`). */
export function isHiddenText(fp: WithKeep, id: string | undefined): boolean {
  return !!id && (keepOf(fp).hidden_texts ?? []).includes(id);
}

/** The outline face a text of the footprint is set in (`TextExtra::face`, the file's `(font (face ..))`); null for KiCad's stroke font. */
export function textFaceOf(fp: WithKeep, id: string | undefined): string | null {
  return (id && keepOf(fp).text_extras?.find((x) => x.id === id)?.face) || null;
}

/** `BOARD_ITEM::IsLocked` of a graphic or text of the footprint. */
export function isLockedItem(fp: WithKeep, id: string | undefined): boolean {
  return !!id && (keepOf(fp).locked ?? []).includes(id);
}

/** `ENUM_MAP<LINE_STYLE>` as the shape dialog's "Line style:" offers it (`DIALOG_SHAPE_PROPERTIES`, `m_lineStyleCombo`: KiCad's names, the file's tokens). */
export const LINE_STYLE_CHOICES: readonly { value: string; label: string }[] = [
  { value: "solid", label: "Solid" },
  { value: "dash", label: "Dashed" },
  { value: "dot", label: "Dotted" },
  { value: "dash_dot", label: "Dash-Dot" },
  { value: "dash_dot_dot", label: "Dash-Dot-Dot" },
];

/** `ENUM_MAP<UI_FILL_MODE>`: the shape dialog's "Fill:" of a closed shape -- none, solid, and the three hatches. */
export const FILL_CHOICES: readonly { value: "none" | "solid" | "hatch" | "reverse_hatch" | "cross_hatch"; label: string }[] = [
  { value: "none", label: "None" },
  { value: "solid", label: "Solid" },
  { value: "hatch", label: "Hatch" },
  { value: "reverse_hatch", label: "Reverse Hatch" },
  { value: "cross_hatch", label: "Cross-hatch" },
];

/** The fill choice of a graphic (its `filled` flag and its kept hatch). */
export function fillChoiceOf(filled: boolean, hatch: ShapeLook["hatch"]): (typeof FILL_CHOICES)[number]["value"] {
  return hatch ?? (filled ? "solid" : "none");
}

/** The `filled` flag and the hatch a fill choice stands for. */
export function fillOfChoice(choice: (typeof FILL_CHOICES)[number]["value"]): { filled: boolean; hatch: ShapeLook["hatch"] } {
  if (choice === "solid") return { filled: true, hatch: null };
  if (choice === "none") return { filled: false, hatch: null };
  return { filled: false, hatch: choice };
}
