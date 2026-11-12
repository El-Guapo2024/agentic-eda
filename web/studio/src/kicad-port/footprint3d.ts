// What the 3D viewer draws of a footprint besides its models: its fields (the Reference, the Value, the user fields) and the solder paste of its pads.
//
// Ported from 3d-viewer/3d_canvas/create_3Dgraphic_brd_items.cpp (`BOARD_ADAPTER::addFootprintShapes`, `BOARD_ADAPTER::addPads`) and pcbnew/pad.cpp
// (`PAD::GetSolderPasteMargin`) at KiCad 8303b2ad. components/viewer3d/scene.ts builds the meshes from these.
//
// Pure: no three.js, no React (compiled by `npm run test:unit`).
import type { FieldInfo, MaskPaste, Pad, Part } from "../api/types";

/**
 * `BOARD_ADAPTER::addFootprintShapes`' field loop: every field of the footprint with `LAYER_FP_TEXT` (the "footprint_text" row), the Reference only with
 * `LAYER_FP_REFERENCES` ("references"), the Value only with `LAYER_FP_VALUES` ("values"), and only a field that `IsVisible()`. The caller draws each on
 * its own layer, as KiCad draws it once per layer `field->GetLayer() == aLayerId` (a Reference on F.Fab is drawn nowhere, since the 3D viewer has no fab
 * layer), at the field's own position, size, angle and justification.
 */
export function fieldsDrawn3d(part: Pick<Part, "fields">, shown: (row: string) => boolean): FieldInfo[] {
  if (!shown("footprint_text")) return [];
  return (part.fields ?? []).filter((f) => {
    if (f.name === "Reference" && !shown("references")) return false;
    if (f.name === "Value" && !shown("values")) return false;
    return f.visible;
  });
}

/** `KiROUND`: half away from zero. */
const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/** Whether a pad's layer set has a copper layer (`( m_padStack.LayerSet() & LSET::AllCuMask() ).any()`): its `(layers ..)` when the state has them, else its kind's (every kind has copper). */
export function padHasCopper(pad: Pick<Pad, "layers">): boolean {
  return pad.layers ? pad.layers.some((l) => /(^|\.)Cu$/.test(l)) : true;
}

/**
 * The paste margin `BOARD_ADAPTER::addPads` grows a pad by on its footprint's side, per axis of the pad's own frame (nm), or null when the pad is not on that side's paste
 * layer (`if( !pad->IsOnLayer( aLayerId ) ) continue;`).
 *
 * The layer: `pad.layers`, the pad's `(layers ..)` in the footprint's own frame (`F.Paste` is the footprint's side), when the state carries it; else the
 * default of its kind (an SMD pad is on its side's paste, a through-hole or non-plated pad is not), which is all the studio knew before.
 *
 * The margin: `PAD::GetSolderPasteMargin( aLayerId )`. "Pads defined only on mask layers (and perhaps on other tech layers) use the shape defined by the pad settings
 * only": a pad with no copper layer gets none (r7 V7-1). Otherwise the margin is the pad's own (`m_padStack.SolderPasteMargin`), else the footprint's
 * (`GetLocalSolderPasteMargin`), else the board's (`m_SolderPasteMargin`); the ratio the same way; per axis `margin + KiROUND( padSize * ratio )`, no less than minus
 * half the pad (a custom pad excepted). Custom DRC rules (`SOLDER_PASTE_ABS_MARGIN_CONSTRAINT`) are not read here, and the pad's own margin is the one an edit set (a pad's
 * margin from the board's file is not in the state).
 */
export function padPasteMargin3d(pad: Pad, part: Pick<Part, "paste_margin" | "paste_margin_ratio">, board: Pick<MaskPaste, "paste_margin_nm" | "paste_margin_ratio"> | undefined | null): [number, number] | null {
  const onPaste = pad.layers ? pad.layers.includes("F.Paste") : !pad.th && pad.kind !== "non_plated_hole";
  if (!onPaste) return null;
  if (!padHasCopper(pad)) return [0, 0];
  return pasteMarginOf(pad, part, board);
}

/** `PAD::GetSolderPasteMargin` of a pad with copper (the margin part of [`padPasteMargin3d`]): the pad's own, else the footprint's, else the board's, per axis of the pad's frame. */
export function pasteMarginOf(pad: Pad, part: Pick<Part, "paste_margin" | "paste_margin_ratio">, board: Pick<MaskPaste, "paste_margin_nm" | "paste_margin_ratio"> | undefined | null): [number, number] {
  const margin = pad.paste_margin ?? part.paste_margin ?? board?.paste_margin_nm ?? 0;
  const ratio = pad.paste_ratio ?? part.paste_margin_ratio ?? board?.paste_margin_ratio ?? 0;
  const [sx, sy] = pad.size ?? [pad.w, pad.h];
  let mx = margin + kiRound(sx * ratio);
  let my = margin + kiRound(sy * ratio);
  if (pad.shape !== "custom") {
    if (mx < -Math.trunc(sx / 2)) mx = -Math.trunc(sx / 2);
    if (my < -Math.trunc(sy / 2)) my = -Math.trunc(sy / 2);
  }
  return [mx, my];
}

/**
 * [`padPasteMargin3d`] as the box `{ w, h }` of the paste on the board (nm), the margin of each of the pad's own axes going to the board axis it lies along (a quarter turn
 * swaps them; a pad turned by another angle takes the mean of the two). The scene draws the pad's real shape grown by the margin (kicad-port/padShape3d.ts); the box is
 * what the tests and the probe read.
 */
export function padPaste3d(pad: Pad, part: Pick<Part, "paste_margin" | "paste_margin_ratio">, board: Pick<MaskPaste, "paste_margin_nm" | "paste_margin_ratio"> | undefined | null): { w: number; h: number } | null {
  const m = padPasteMargin3d(pad, part, board);
  if (!m) return null;
  const [mx, my] = m;
  const [sx, sy] = pad.size ?? [pad.w, pad.h];
  // Which of the pad's own axes lies along the board's x.
  const straight = Math.abs(pad.w - sx) <= 1 && Math.abs(pad.h - sy) <= 1;
  const turned = Math.abs(pad.w - sy) <= 1 && Math.abs(pad.h - sx) <= 1;
  const [bx, by] = straight ? [mx, my] : turned ? [my, mx] : [(mx + my) / 2, (mx + my) / 2];
  return { w: Math.max(0, pad.w + 2 * bx), h: Math.max(0, pad.h + 2 * by) };
}

/**
 * The sides whose solder mask a pad opens (`BOARD_ADAPTER::addPads` on F.Mask / B.Mask: `if( !pad->IsOnLayer( aLayerId ) ) continue;`): its `(layers ..)` in the footprint's
 * frame (`F.Mask` the footprint's side, `B.Mask` the other, `*.Mask` both), else its kind's (an SMD pad its side's mask, a hole both).
 */
export function padMaskSides(pad: Pick<Pad, "layers" | "th">, side: "top" | "bottom"): Array<"top" | "bottom"> {
  const other = side === "top" ? "bottom" : "top";
  if (!pad.layers) return pad.th ? [side, other] : [side];
  const out = new Set<"top" | "bottom">();
  for (const l of pad.layers) {
    if (l === "*.Mask" || l === "F&B.Mask") {
      out.add(side);
      out.add(other);
    } else if (l === "F.Mask") out.add(side);
    else if (l === "B.Mask") out.add(other);
  }
  return [...out];
}

/**
 * `PAD::GetSolderMaskExpansion`: no expansion for a pad with no copper layer ("Pads defined only on mask layers ... use the shape defined by the pad settings only"); else the
 * pad's own margin, else the footprint's (`GetLocalSolderMaskMargin`), else the board's (`m_SolderMaskExpansion`, Board Setup's "Solder mask expansion"), "ensure mask have
 * a size always >= 0": no less than minus half the pad's smaller side. Custom DRC rules are not read.
 */
export function padMaskMargin3d(pad: Pad, part: Pick<Part, "mask_margin"> | undefined | null, board: Pick<MaskPaste, "expansion_nm"> | undefined | null): number {
  if (!padHasCopper(pad)) return 0;
  const margin = pad.mask_margin ?? part?.mask_margin ?? board?.expansion_nm ?? 0;
  if (margin < 0) {
    const [sx, sy] = pad.size ?? [pad.w, pad.h];
    const minsize = -Math.trunc(Math.min(sx, sy) / 2);
    if (margin < minsize) return minsize;
  }
  return margin;
}
