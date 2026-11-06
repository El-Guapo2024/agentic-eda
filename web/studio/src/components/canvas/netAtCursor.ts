// Port of board_inspection_tool.cpp's BOARD_INSPECTION_TOOL::highlightNet, its cursor half (KiCad 8303b2ad): the net of the item under the cursor, as
// KiCad's collector finds it (r7 C7-11):
//
//   GENERAL_COLLECTORS_GUIDE guide = m_frame->GetCollectorsGuide();
//   guide.SetIgnoreZoneFills( false ); guide.SetIgnoreNoNets( true ); guide.SetPreferredLayer( activeLayer );
//   collector.Collect( board, { PCB_PAD_T, PCB_VIA_T, PCB_TRACE_T, PCB_ARC_T, PCB_SHAPE_T }, aPosition, guide );
//   if( collector.GetCount() == 0 ) collector.Collect( board, { PCB_ZONE_T }, aPosition, guide );
//   ... non-copper items and, in high contrast, items off the contrast layer removed; net = collector[0]->GetNetCode();
//
// `GENERAL_COLLECTOR::Inspect`: each item is asked with its own `HitTest( pos, accuracy )` -- a pad its shape (`PAD::HitTest`), a via its circle, a track
// its width -- `accuracy` being the guide's (`KiROUND( 5 * m_onePixelInIU )`: five pixels); an item with no net is skipped (`IgnoreNoNets`), and so is one
// on a hidden layer. The items on the preferred (active) layer come first, in the order of the type list (pads, then vias, then tracks and arcs, then
// shapes), then the ones on the other visible layers (`Append2nd`). A zone is hit on a corner (`accuracy * 2`) or an edge, or inside its fill on a
// visible layer (`HitTestFilledArea`, the zone fill shown), not inside an unfilled outline.
import type { BoardState, FillZone } from "../../api/types";
import { layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";
import { distToPolyline, pointInPolygon, shapeHitDistance } from "./itemHitTest";
import { padHitDistance } from "./selectionCandidates";

/** What the guide knows of the view: the active (preferred) layer as a state key, the visible layers, high contrast, and the zone fills shown. */
export interface NetPickView {
  activeLayer?: string | null;
  layerVisible?: Readonly<Record<string, boolean>>;
  highContrast?: boolean;
  fills?: readonly FillZone[] | null;
}

/** `GENERAL_COLLECTORS_GUIDE::Accuracy()`: five pixels, in nanometres, for a view `scale` (pixels per nanometre). */
export function collectorAccuracyNm(scale: number): number {
  return Math.round(5 / (scale || 1));
}

/** The net of the item under (xNm, yNm), within `accuracyNm` ([`collectorAccuracyNm`]); `null` when no item with a net is there. */
export function findNetAtCursor(board: BoardState, xNm: number, yNm: number, accuracyNm: number, view: NetPickView = {}): string | null {
  const copper = (board.layers ?? ["F.Cu", "B.Cu"]).filter((l) => l.endsWith(".Cu"));
  const visible = (layer: string) => !view.layerVisible || layerIsVisible(view.layerVisible, layer);
  const preferred = (layers: readonly string[]) => view.activeLayer != null && layers.some((l) => layerStateKey(l) === view.activeLayer);
  // the high-contrast filter after the collector: an item off the primary contrast layer goes
  const contrastOk = (layers: readonly string[]) => !view.highContrast || view.activeLayer == null || preferred(layers);
  const primary: string[] = [];
  const secondary: string[] = [];
  const take = (net: string | null | undefined, layers: readonly string[], hit: boolean) => {
    if (!net || !hit) return; // `IgnoreNoNets`
    const shown = layers.filter((l) => l.endsWith(".Cu") && visible(l));
    if (shown.length === 0 || !contrastOk(layers)) return;
    (preferred(layers) ? primary : secondary).push(net);
  };

  for (const part of board.parts) {
    if (!part.placed) continue;
    const side = part.side === "bottom" ? "B.Cu" : "F.Cu";
    for (const pad of part.pads ?? []) take(pad.net, pad.th ? copper : [side], padHitDistance(pad, xNm, yNm) <= accuracyNm);
  }
  for (const v of board.routing?.vias ?? []) {
    const from = copper.indexOf(v.from);
    const to = copper.indexOf(v.to);
    const span = from >= 0 && to >= 0 ? copper.slice(Math.min(from, to), Math.max(from, to) + 1) : copper;
    take(v.net, span, Math.hypot(xNm - v.x, yNm - v.y) - v.d / 2 <= accuracyNm);
  }
  for (const t of board.routing?.tracks ?? []) take(t.net, [t.layer], distToPolyline(xNm, yNm, t.pts) - t.width / 2 <= accuracyNm);
  for (const s of board.drawings?.shapes ?? []) {
    if (s.layer.endsWith(".Cu")) take(s.net, [s.layer], shapeHitDistance(s, xNm, yNm) <= accuracyNm);
  }
  const first = primary[0] ?? secondary[0];
  if (first) return first;

  const zonePrimary: string[] = [];
  const zoneSecondary: string[] = [];
  for (const z of board.routing?.zones ?? []) {
    if (!z.net) continue;
    const layers = z.layers && z.layers.length > 0 ? z.layers : [z.layer];
    if (!layers.some((l) => l.endsWith(".Cu") && visible(l)) || !contrastOk(layers)) continue;
    const rings = [z.outline, ...(z.holes ?? [])].filter((r) => r.length >= 2);
    const onRim = rings.some((r) => r.some((c) => Math.hypot(c[0] - xNm, c[1] - yNm) <= 2 * accuracyNm) || distToPolyline(xNm, yNm, [...r, r[0]!]) <= accuracyNm);
    const inFill = (view.fills ?? []).some((f) => f.id === z.id && f.filled && visible(f.layer) && f.fragments.some((ring) => ring.length >= 3 && pointInPolygon(xNm, yNm, ring)));
    if (!onRim && !inFill) continue;
    (preferred(layers) ? zonePrimary : zoneSecondary).push(z.net);
  }
  return zonePrimary[0] ?? zoneSecondary[0] ?? null;
}
