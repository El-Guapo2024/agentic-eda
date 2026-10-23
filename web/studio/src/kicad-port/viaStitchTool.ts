// What KiCad 10's Draw Via Stitch Area (`PCB_ACTIONS::drawViaStitchArea`, `DRAWING_TOOL::DrawZone` in `ZONE_MODE::STITCH`, `ZONE_CREATE_HELPER::commitZone`) starts a via stitching with:
// the outline drawn, and `PCB_VIA_STITCH::InitializeDefaults` -- the board's current via size and drill (0.6 and 0.3 mm when it has none) and the default pitch (twice the via, at
// least 2 mm). No net: `ZONE_CREATE_HELPER::commitZone` gives the stitcher the outline only (`SetOutline`, `InitializeDefaults`), and the properties dialog that opens before any
// via is made is where the net is picked.
import type { BoardViaStitch } from "../api/pcbItemTypes";
import { defaultPitchUm } from "./viaStitch";

export function newViaStitch(outline: readonly (readonly [number, number])[], currentViaSizeUm: number, currentViaDrillUm: number): BoardViaStitch {
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
    net: "",
    via: { size_um: size, drill_um: drill, from_layer: "F.Cu", to_layer: "B.Cu" },
  };
}
