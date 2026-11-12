// The order the board editor draws its layers in: `GAL_LAYER_ORDER` and `PCB_DRAW_PANEL_GAL::SetTopLayer` (pcbnew/pcb_draw_panel_gal.cpp, KiCad 25ec2335 --
// the order of 8303b2ad with KiCad 10's sub-grid, via stitching, constraint and router preview layers), and `VIEW::SetTopLayer` / `SortOrderedLayers`
// (common/view/view.cpp): every layer has a rendering order, its index in `GAL_LAYER_ORDER` (the first on top); the layers made "top" have
// `TOP_LAYER_MODIFIER` (minus `VIEW_MAX_LAYERS`) added, which takes them over every other layer and keeps their order among themselves; the layers are
// drawn from the highest order down. UI-LEFT-2 PP3: the painter draws a pass per layer of this list (components/canvas/painter.ts `paintBoard`).
//
// A layer of the list is a board layer by name ("F.Cu", "F.SilkS", "User.3"), one of the virtual layers KiCad keeps per board layer -- `zone:L`
// (`ZONE_LAYER_FOR`), `pad:L` (`PAD_COPPER_LAYER_FOR`), `via:L` (`VIA_COPPER_LAYER_FOR`), `clearance:L` (`CLEARANCE_LAYER_FOR`), `netnames:L`
// (`NETNAMES_LAYER_INDEX`), `point:L` (`POINT_LAYER_FOR`), `bitmap:L` (`BITMAP_LAYER_FOR`) -- or a GAL layer by its KiCad name ("LAYER_VIA_HOLES").

/** `LAYER_UI_START` .. `LAYER_UI_START + 9`, one name for the ten. */
const UI = "LAYER_UI";

const FRONT = ["F.Cu", "F.Mask", "F.SilkS", "F.Paste", "F.Adhes", "F.CrtYd", "F.Fab"] as const;
const BACK = ["B.Cu", "B.Mask", "B.SilkS", "B.Paste", "B.Adhes", "B.CrtYd", "B.Fab"] as const;
const DRAWINGS = ["Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin"] as const;
const users = (): string[] => Array.from({ length: 45 }, (_, i) => `User.${i + 1}`);
const inner = (): string[] => Array.from({ length: 30 }, (_, i) => `In${i + 1}.Cu`);

const isCopper = (layer: string): boolean => /\.Cu$/.test(layer);
/** `IsFrontLayer`: F.Cu and the front technical layers. */
const isFront = (layer: string): boolean => (FRONT as readonly string[]).includes(layer);
/** `IsBackLayer`. */
const isBack = (layer: string): boolean => (BACK as readonly string[]).includes(layer);
const isBitmap = (layer: string): boolean => layer.startsWith("bitmap:");

/** `GAL_LAYER_ORDER` of KiCad 25ec2335, the first on top; `LAYER_FP_TEXT`, `LAYER_FP_REFERENCES` and `LAYER_FP_VALUES` are visibility switches, not drawn on. */
export function galLayerOrderTopFirst(): string[] {
  const out: string[] = [UI, "LAYER_GP_OVERLAY", "LAYER_ROUTER_PREVIEW_COLLISION", "LAYER_SELECT_OVERLAY", "LAYER_CONFLICTS_SHADOW"];
  out.push("LAYER_DRC_HIGHLIGHTED", "LAYER_DRC_ERROR", "LAYER_DRC_WARNING", "LAYER_DRC_EXCLUSION", "LAYER_MARKER_SHADOWS");
  out.push("LAYER_PAD_NETNAMES", "LAYER_VIA_NETNAMES");
  for (const l of [...DRAWINGS, ...users()]) out.push(l, `zone:${l}`, `point:${l}`);
  out.push("point:F.Cu");
  out.push("LAYER_SUBGRIDS");
  out.push("LAYER_RATSNEST", "LAYER_ANCHOR", "LAYER_POINTS", "LAYER_VIA_STITCHING", "LAYER_LOCKED_ITEM_SHADOW", "LAYER_CONSTRAINT_SHADOW");
  out.push("LAYER_VIA_HOLES", "LAYER_VIA_HOLEWALLS", "LAYER_PAD_PLATEDHOLES", "LAYER_PAD_HOLEWALLS", "LAYER_NON_PLATEDHOLES");
  out.push("LAYER_VIA_THROUGH", "LAYER_VIA_BLIND", "LAYER_VIA_BURIED", "LAYER_VIA_MICROVIA");
  out.push("LAYER_PAD_FR_NETNAMES", "netnames:F.Cu", "pad:F.Cu", "via:F.Cu", "clearance:F.Cu");
  // a front or back technical layer's points are not in the list (`POINT_LAYER_FOR( F_Mask )` .. keep the order they were made in): here, over their layer
  for (const l of FRONT) out.push(...(isCopper(l) ? [] : [`point:${l}`]), l, `zone:${l}`);
  for (const l of inner()) out.push(`netnames:${l}`, `pad:${l}`, `via:${l}`, `clearance:${l}`, `point:${l}`, l, `zone:${l}`);
  out.push("LAYER_PAD_BK_NETNAMES", "netnames:B.Cu", "pad:B.Cu", "via:B.Cu", "clearance:B.Cu", "point:B.Cu");
  for (const l of BACK) out.push(...(isCopper(l) ? [] : [`point:${l}`]), l, `zone:${l}`);
  out.push("LAYER_ROUTER_PREVIEW_SHADOW");
  for (const l of [...DRAWINGS, ...users(), ...FRONT, ...inner(), ...BACK]) out.push(`bitmap:${l}`);
  out.push("LAYER_BOARD_OUTLINE_AREA", "LAYER_DRAWINGSHEET");
  return out;
}

/** `SetTopLayer`'s "Layers that should always have on-top attribute enabled" (25ec2335), and the UI layers. */
const ALWAYS_ON_TOP = [
  "LAYER_VIA_THROUGH", "LAYER_VIA_BLIND", "LAYER_VIA_BURIED", "LAYER_VIA_MICROVIA", "LAYER_VIA_HOLES", "LAYER_VIA_HOLEWALLS",
  "LAYER_PAD_PLATEDHOLES", "LAYER_PAD_HOLEWALLS", "LAYER_NON_PLATEDHOLES", "LAYER_PAD_NETNAMES", "LAYER_VIA_NETNAMES", "LAYER_SELECT_OVERLAY",
  "LAYER_GP_OVERLAY", "LAYER_ROUTER_PREVIEW_SHADOW", "LAYER_ROUTER_PREVIEW_COLLISION", "LAYER_RATSNEST", "LAYER_ANCHOR", "LAYER_DRC_HIGHLIGHTED",
  "LAYER_DRC_ERROR", "LAYER_DRC_WARNING", "LAYER_DRC_EXCLUSION", "LAYER_MARKER_SHADOWS", "LAYER_CONFLICTS_SHADOW", "LAYER_CONSTRAINT_SHADOW", UI,
];

/**
 * The layers back to front -- the order a painter draws them in -- with `active` the active layer (`PCB_EDIT_FRAME::SetActiveLayer` ->
 * `SetHighContrastLayer` -> `SetTopLayer( aLayer )`, whatever the contrast mode):
 * - the layers always on top: the vias' and pads' holes and net names, the ratsnest, the anchors, the DRC markers and the overlays;
 * - the active layer and its zones, points and image; with a copper layer its pads', vias' and clearance outlines and its net names too;
 * - with a front (back) layer active, every front (back) layer and their zones, pads, vias, clearances and points, and the side's pad net names;
 * - an active technical layer goes just under the marker shadows -- over the vias and pads ("Move the active layer to the top of the stack but below all
 *   the overlay layers") -- its zones and points under it, the pad and via net names under those;
 * - the images stay under every other layer, the active layer's on top of the other images (`BITMAP_LAYER_FOR`'s "Gross hack").
 * Layers of the same order (a rare tie of `SetLayerOrder`) keep the list's order.
 */
export function galDrawOrder(active: string | null | undefined): string[] {
  const list = galLayerOrderTopFirst();
  const MOD = -10_000; // `TOP_LAYER_MODIFIER = -VIEW_MAX_LAYERS`: only that it is more than the list is long matters
  const order = new Map<string, number>();
  list.forEach((l, i) => order.set(l, isBitmap(l) ? i - MOD : i));
  const top = new Set<string>();
  const raise = (l: string) => {
    if (top.has(l) || !order.has(l)) return;
    top.add(l);
    order.set(l, order.get(l)! + MOD);
  };
  const layer = active || "F.Cu";
  raise(layer);
  for (const l of ALWAYS_ON_TOP) raise(l);
  const extra = isFront(layer) ? [...FRONT, "LAYER_PAD_FR_NETNAMES", "netnames:F.Cu"] : isBack(layer) ? [...BACK, "LAYER_PAD_BK_NETNAMES", "netnames:B.Cu"] : null;
  if (extra) {
    for (const l of extra) {
      raise(l);
      if (!l.startsWith("LAYER_") && !l.startsWith("netnames:")) for (const v of ["zone", "pad", "via", "clearance", "point"]) raise(`${v}:${l}`);
    }
    if (!isCopper(layer)) {
      const m = order.get("LAYER_MARKER_SHADOWS")!;
      order.set(layer, m + 1);
      order.set(`zone:${layer}`, m + 2);
      order.set(`point:${layer}`, m + 3);
      order.set("LAYER_PAD_NETNAMES", m + 4);
      order.set("LAYER_VIA_NETNAMES", m + 5);
    }
  }
  if (isCopper(layer)) for (const v of ["zone", "pad", "via", "clearance", "netnames"]) raise(`${v}:${layer}`);
  raise(`point:${layer}`);
  raise(`bitmap:${layer}`);
  const at = new Map(list.map((l, i) => [l, i]));
  return [...list].sort((a, b) => order.get(b)! - order.get(a)! || at.get(b)! - at.get(a)!);
}
