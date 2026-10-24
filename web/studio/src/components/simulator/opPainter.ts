// The operating point drawn on the schematic canvas (`SCH_PAINTER` on LAYER_OP_VOLTAGES and LAYER_OP_CURRENTS: the stroke font, demi-bold, knocked out of
// the background). Where the labels go is kicad-port/sim/opOverlay.ts's.
import type { Schematic } from "../../api/types";
import { opLabels, opTextSize } from "../../kicad-port/sim/opOverlay";
import { pinItems } from "../../kicad-port/schPins";
import { getSimUi } from "../../state/simulator";
import { drawStrokeText } from "../text/strokeFont";
import { layerColor } from "../canvas/layers";

/** Draw the labels on a canvas already set to sheet coordinates (micrometres), `scale` pixels per micrometre; `background` is the sheet's colour. */
export function paintOperatingPoints(ctx: CanvasRenderingContext2D, scale: number, sch: Schematic, background: string): void {
  const ui = getSimUi();
  if (ui.opPoints.size === 0 || (!ui.showOpVoltages && !ui.showOpCurrents)) return;
  const pins = pinItems(sch).map((p) => ({ reference: p.reference, number: p.number, tip: p.tip, root: p.root, net: p.net }));
  const labels = opLabels({ wires: sch.wires, pins, circuit: ui.frame.circuit, points: ui.opPoints, voltages: ui.showOpVoltages, currents: ui.showOpCurrents, scale });
  const size = opTextSize(scale);
  // `GetPenSizeForDemiBold`: a sixth of the size.
  const thickness = size / 6;
  for (const l of labels) {
    const color = layerColor(l.kind === "voltage" ? "LAYER_OP_VOLTAGES" : "LAYER_OP_CURRENTS");
    // the baseline from the vertical justification (bottom: on it; centre: half the size below)
    const dy = l.v === "center" ? size / 2 : 0;
    const angleRad = l.vertical ? -Math.PI / 2 : 0;
    const [x, y] = l.vertical ? [l.x + dy, l.y] : [l.x, l.y + dy];
    const opts = { sizeNm: size, justify: l.h, angleRad } as const;
    // knockout: the text first in the background's colour, wider
    drawStrokeText(ctx, l.text, x, y, { ...opts, thicknessNm: thickness * 3, color: background });
    drawStrokeText(ctx, l.text, x, y, { ...opts, thicknessNm: thickness, color });
  }
}
