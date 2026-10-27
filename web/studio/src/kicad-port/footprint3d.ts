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

/**
 * The solder paste `BOARD_ADAPTER::addPads` draws for a pad on its footprint's side, as a box `{ w, h }` on the board (nm), or null when the pad is not on
 * that side's paste layer (`if( !pad->IsOnLayer( aLayerId ) ) continue;`).
 *
 * The layer: `pad.layers`, the pad's `(layers ..)` in the footprint's own frame (`F.Paste` is the footprint's side), when the state carries it; else the
 * default of its kind (an SMD pad is on its side's paste, a through-hole or non-plated pad is not), which is all the studio knew before.
 *
 * The size: `createPadWithMargin( pad, .., GetSolderPasteMargin( aLayerId ) )`. `PAD::GetSolderPasteMargin`: the margin is the pad's own
 * (`m_padStack.SolderPasteMargin`), else the footprint's (`GetLocalSolderPasteMargin`), else the board's (`m_SolderPasteMargin`); the ratio the same way;
 * per axis `margin + KiROUND( padSize * ratio )`, no less than minus half the pad (a custom pad excepted). Custom DRC rules (`SOLDER_PASTE_ABS_MARGIN_CONSTRAINT`)
 * are not read here, and the pad's own margin is the one an edit set (a pad's margin from the board's file is not in the state).
 *
 * The axes: KiCad grows the pad in its own frame; the scene draws the pad's box on the board (`w`, `h`), so each axis's margin goes to the board axis the
 * pad's own axis lies along (a quarter turn swaps them; a pad turned by another angle takes the mean of the two).
 */
export function padPaste3d(pad: Pad, part: Pick<Part, "paste_margin" | "paste_margin_ratio">, board: Pick<MaskPaste, "paste_margin_nm" | "paste_margin_ratio"> | undefined | null): { w: number; h: number } | null {
  const onPaste = pad.layers ? pad.layers.includes("F.Paste") : !pad.th && pad.kind !== "non_plated_hole";
  if (!onPaste) return null;
  const margin = pad.paste_margin ?? part.paste_margin ?? board?.paste_margin_nm ?? 0;
  const ratio = pad.paste_ratio ?? part.paste_margin_ratio ?? board?.paste_margin_ratio ?? 0;
  const [sx, sy] = pad.size ?? [pad.w, pad.h];
  let mx = margin + kiRound(sx * ratio);
  let my = margin + kiRound(sy * ratio);
  if (pad.shape !== "custom") {
    if (mx < -Math.trunc(sx / 2)) mx = -Math.trunc(sx / 2);
    if (my < -Math.trunc(sy / 2)) my = -Math.trunc(sy / 2);
  }
  // Which of the pad's own axes lies along the board's x.
  const straight = Math.abs(pad.w - sx) <= 1 && Math.abs(pad.h - sy) <= 1;
  const turned = Math.abs(pad.w - sy) <= 1 && Math.abs(pad.h - sx) <= 1;
  const [bx, by] = straight ? [mx, my] : turned ? [my, mx] : [(mx + my) / 2, (mx + my) / 2];
  return { w: Math.max(0, pad.w + 2 * bx), h: Math.max(0, pad.h + 2 * by) };
}
