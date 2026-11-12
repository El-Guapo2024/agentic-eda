// The via a KiCad 10 via stitching makes copies of (`PCB_VIA_STITCH::m_viaTemplate`), as Via Stitching Properties' Configure edits it: `OnViaConfigureClicked` opens
// `DIALOG_TRACK_VIA_PROPERTIES` in its standalone form on the template (pcbnew/dialogs/dialog_via_stitch_properties.cpp and dialog_track_via_properties.cpp of KiCad 25ec2335),
// and its OK becomes the stitching's template on the stitching dialog's OK (`SetTemplateItem( "via", .. )`).
//
// UI-LEFT-2 PP22: the dialog is the studio's Track & Via Properties itself (components/TrackViaPropertiesDialog.tsx, kicad-port/trackViaDialog.ts) on a board that holds the
// template as its one via ([`templateAsVia`], [`templateBoard`]), so it has every row KiCad's standalone dialog has -- Net, Locked, the position, Pre-defined sizes, the
// padstack, Via type and layers, Annular rings, Protection features, Backdrill, Hole Post-machining and Teardrops; [`templateFromDialog`] is its `TransferDataFromWindow`
// on the standalone via: what changed set over the template's text (`kind`, `annular`, `props`, `teardrops`, `net`, `locked`; crates/kicad/src/via_stitch_io.rs). The
// smaller form below (`viaTemplateForm`, `viaTemplateFrom`) is kept for its tests and callers.
//
// Pure: no React, no store. Unit-tested in viaTemplate.test.ts.
import type { BoardViaStitch } from "../api/pcbItemTypes";
import type { BoardState, Nm, UnconnectedLayers, Via } from "../api/types";
import { validateViaParameters } from "./routerVia";
import { viaPropsAfter, type TrackViaForm, type ViaType } from "./trackViaDialog";
import { applyTeardropForm, teardropFormChanged, viaTeardrops } from "./teardropParams";

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

/** The id the template has as the one via of [`templateBoard`]. */
export const TEMPLATE_VIA_ID = "via_stitch_template";

/** The template as a via of the board (`DIALOG_TRACK_VIA_PROPERTIES( aParent, aStandaloneVia )`): what Configure set, else what its text says (`via_view`, the server's reading). */
export function templateAsVia(v: StitchVia, view?: BoardViaStitch["via_view"]): Via {
  const type = templateViaType(v);
  const rings = templateRings(v);
  return {
    id: TEMPLATE_VIA_ID,
    net: v.net ?? view?.net ?? "",
    x: 0,
    y: 0,
    d: v.size_nm,
    drill: v.drill_nm,
    from: v.from_layer,
    to: v.to_layer,
    type: type === "through" ? null : type,
    unconnected: rings === "keep_all" ? null : rings,
    props: v.props ?? view?.props ?? null,
    teardrops: v.teardrops ?? view?.teardrops ?? null,
  };
}

/** Whether the template is locked: what Configure set, else what its text says. */
export function templateLocked(v: StitchVia, view?: BoardViaStitch["via_view"]): boolean {
  return v.locked ?? view?.locked ?? false;
}

/** `board` with the template as its one via and no tracks: what the standalone dialog edits (`m_viaStandaloneSelection`). */
export function templateBoard(board: BoardState, via: Via, locked: boolean): BoardState {
  const routing = board.routing ?? ({ tracks: [], vias: [], zones: [], track_width_presets: [], via_presets: [] } as unknown as NonNullable<BoardState["routing"]>);
  return { ...board, routing: { ...routing, tracks: [], vias: [via] }, locked: locked ? [via.id] : [] };
}

/**
 * `TransferDataFromWindow` on the standalone via (`m_standalone`: nothing committed): `ValidateViaParameters`' message (word for word), or the template with what the dialog
 * changed set over its text -- the size and drill, the type, layers and annular rings, the padstack, protection, backdrill and post-machining (`props`, the via's whole
 * `ViaProps` as `viaPropsAfter` makes them), the teardrops, the net and the lock. A field the dialog left as it opened leaves the template's text to say it. The position is
 * the template's own and is not kept (a via the stitching makes is put on its cell).
 */
export function templateFromDialog(v: StitchVia, board: BoardState, before: TrackViaForm, form: TrackViaForm, copper: readonly string[]): { via: StitchVia } | { error: string } {
  const via = board.routing!.vias[0]!;
  const why = validateViaParameters(form.diameter, form.drill);
  if (why) return { error: why };
  const type = form.viaType ?? templateViaType(v);
  if (type !== "through" && form.viaFrom != null && form.viaFrom === form.viaTo) return { error: "Via start layer and end layer cannot be the same" };
  const [from, to] = type === "through" ? [copper[0] ?? "F.Cu", copper[copper.length - 1] ?? "B.Cu"] : [form.viaFrom ?? v.from_layer, form.viaTo ?? v.to_layer];
  const out: StitchVia = { ...v, size_nm: form.diameter!, drill_nm: form.drill!, from_layer: from, to_layer: to };
  if (v.kind || type !== templateViaType(v)) out.kind = type;
  const rings = form.rings ?? templateRings(v);
  if (v.annular || rings !== templateRings(v)) out.annular = rings;
  const props = viaPropsAfter(via, before, form);
  if (v.props || JSON.stringify(sortedKeys(props)) !== JSON.stringify(sortedKeys(via.props ?? {}))) out.props = props;
  if (teardropFormChanged(before.teardrops, form.teardrops)) out.teardrops = applyTeardropForm(viaTeardrops(board, via), before.teardrops, form.teardrops);
  if (form.net != null && form.net !== before.net) out.net = form.net;
  if (form.locked != null && form.locked !== before.locked) out.locked = form.locked;
  return { via: out };
}

function sortedKeys(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(sortedKeys);
  if (v && typeof v === "object") return Object.fromEntries(Object.keys(v as object).sort().map((k) => [k, sortedKeys((v as Record<string, unknown>)[k])]));
  return v;
}
