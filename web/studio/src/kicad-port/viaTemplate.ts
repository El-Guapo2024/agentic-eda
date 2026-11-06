// The via a KiCad 10 via stitching makes copies of (`PCB_VIA_STITCH::m_viaTemplate`), as Via Stitching Properties' Configure edits it: `OnViaConfigureClicked` opens
// `DIALOG_TRACK_VIA_PROPERTIES` in its standalone form on the template (pcbnew/dialogs/dialog_via_stitch_properties.cpp and dialog_track_via_properties.cpp of KiCad 25ec2335),
// and its OK becomes the stitching's template on the stitching dialog's OK (`SetTemplateItem( "via", .. )`).
//
// The rows here are the via rows the studio's Track & Via Properties has (kicad-port/trackViaDialog.ts): Via type, Start layer and End layer (a through via spans F.Cu to B.Cu
// and the two are disabled, `onViaEdit`), Diameter, Hole (`PCB_VIA::ValidateViaParameters`) and Annular rings (shown when the via spans more than one layer pair,
// `getLayerDepth() > 1`). The template's other tokens -- tenting, covering, plugging, filling, capping, backdrill, post-machining, teardrops, its padstack -- are kept as the
// file had them and copied to every via (crates/kicad/src/via_stitch_io.rs `template_extra`).
//
// Pure: no React, no store. Unit-tested in viaTemplate.test.ts.
import type { BoardViaStitch } from "../api/pcbItemTypes";
import type { Nm, UnconnectedLayers } from "../api/types";
import { validateViaParameters } from "./routerVia";
import type { ViaType } from "./trackViaDialog";

export type StitchVia = NonNullable<BoardViaStitch["via"]>;

/** The template's via type: what Configure set, else the word after `(via` in its file text (a through via has none). */
export function templateViaType(v: StitchVia): ViaType {
  if (v.kind) return v.kind;
  const m = /^\(via\s+(blind|buried|micro)\b/.exec(v.text ?? "");
  return m ? (m[1] as ViaType) : "through";
}

/** The template's annular rings (`UNCONNECTED_LAYER_MODE`): what Configure set, else its text's `(remove_unused_layers ..)`, `(keep_end_layers ..)`, `(start_end_only ..)`. */
export function templateRings(v: StitchVia): UnconnectedLayers {
  if (v.annular) return v.annular;
  const t = v.text ?? "";
  if (/\(start_end_only yes\)/.test(t)) return "start_end_only";
  if (/\(remove_unused_layers yes\)/.test(t)) return /\(keep_end_layers yes\)/.test(t) ? "remove_except_start_and_end" : "remove_all";
  return "keep_all";
}

/** What the template dialog shows (`TransferDataToWindow` of the one via). */
export interface ViaTemplateForm {
  type: ViaType;
  from: string;
  to: string;
  diameterNm: number | null;
  drillNm: number | null;
  rings: UnconnectedLayers;
}

export function viaTemplateForm(v: StitchVia): ViaTemplateForm {
  return { type: templateViaType(v), from: v.from_layer, to: v.to_layer, diameterNm: v.size_nm, drillNm: v.drill_nm, rings: templateRings(v) };
}

/** `onViaEdit`: a through via's pair is the board's outer layers. */
export function withViaType(f: ViaTemplateForm, type: ViaType, copper: readonly string[]): ViaTemplateForm {
  if (type !== "through") return { ...f, type };
  return { ...f, type, from: copper[0] ?? "F.Cu", to: copper[copper.length - 1] ?? "B.Cu" };
}

/**
 * `TransferDataFromWindow` on the standalone via: `ValidateViaParameters`' message (word for word), or the template with the dialog's values -- the type and the annular rings
 * as overrides of the text (`kind`, `annular`), the layers a through via has.
 */
export function viaTemplateFrom(v: StitchVia, f: ViaTemplateForm, copper: readonly string[]): { via: StitchVia } | { error: string } {
  const why = validateViaParameters(f.diameterNm as Nm | null, f.drillNm as Nm | null);
  if (why) return { error: why };
  if (f.type !== "through" && f.from === f.to) return { error: "Via start layer and end layer cannot be the same" };
  const [from, to] = f.type === "through" ? [copper[0] ?? "F.Cu", copper[copper.length - 1] ?? "B.Cu"] : [f.from, f.to];
  const out: StitchVia = { ...v, size_nm: f.diameterNm!, drill_nm: f.drillNm!, from_layer: from, to_layer: to };
  // Only what the dialog changed overrides the file's text, so a template opened and confirmed is written as it was.
  if (v.kind || f.type !== templateViaType(v)) out.kind = f.type;
  if (v.annular || f.rings !== templateRings(v)) out.annular = f.rings;
  return { via: out };
}
