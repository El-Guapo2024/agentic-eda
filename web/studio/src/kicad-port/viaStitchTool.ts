// What KiCad 10's Draw Via Stitch Area (`PCB_ACTIONS::drawViaStitchArea`, `DRAWING_TOOL::DrawZone` in `ZONE_MODE::STITCH`, `ZONE_CREATE_HELPER::commitZone`) starts a via stitching with:
// the outline drawn, and `PCB_VIA_STITCH::InitializeDefaults` -- the board's current via size and drill (0.6 and 0.3 mm when it has none) and the default pitch (twice the via, at
// least 2 mm). The net is the one the zone tool would take: the highlighted net, else the first selected item's (`createNewZone`).
import type { BoardViaStitch } from "../api/pcbItemTypes";
import { defaultPitchUm } from "./viaStitch";

export function newViaStitch(outline: readonly (readonly [number, number])[], currentViaSizeUm: number, currentViaDrillUm: number, net: string): BoardViaStitch {
  const size = currentViaSizeUm > 0 ? currentViaSizeUm : 600;
  const drill = currentViaDrillUm > 0 ? currentViaDrillUm : 300;
  return {
    id: "",
    layer: "F.Cu",
    name: "Via Stitching",
    outline: outline.map(([x, y]) => ({ x, y })),
    pitch_um: defaultPitchUm(size),
    layout: "plain",
    mode: "stitch",
    seed: 0,
    net,
    via: { size_um: size, drill_um: drill, from_layer: "F.Cu", to_layer: "B.Cu" },
  };
}

/** `createNewZone`'s net: the highlighted net, else the net of the first selected item that has one. */
export function netForNewStitch(state: { netHighlight?: string | null; selection: ReadonlySet<string> }, api: { trackById(id: string): { net: string } | undefined; viaById(id: string): { net: string } | undefined; zoneById(id: string): { net: string } | undefined }): string {
  if (state.netHighlight) return state.netHighlight;
  const first = [...state.selection][0];
  if (!first) return "";
  return api.trackById(first)?.net ?? api.viaById(first)?.net ?? api.zoneById(first)?.net ?? "";
}
