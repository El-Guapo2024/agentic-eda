// KiCad 10's via stitchings on the canvas (kicad-port/viaStitch.ts): `PCB_VIA_STITCH::ViewDraw` on `LAYER_VIA_STITCHING` -- the outline as a 0.1 mm polyline in the Via Stitching
// colour of the theme, and hairline ticks every 2 mm along each edge, 0.5 mm long, pointing in ("It kind of looks like a sewing pattern with the ticks, so it fits."). The vias it made are
// the board's own vias and are drawn as any via is. A selected stitching is drawn in the selection colour (`PCB_RENDER_SETTINGS::GetColor` of a selected item).
import type { BoardState } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { OUTLINE_WIDTH_NM, stitchTicks } from "../../kicad-port/viaStitch";
import { hairlineNm } from "./view";
import { layerColor } from "./layers";

export function drawViaStitches(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, layerVisible: Readonly<Record<string, boolean>>, selection: ReadonlySet<string>): void {
  if (layerVisible["obj:via_stitching"] === false) return;
  for (const v of board.drawings?.via_stitches ?? []) {
    if (v.outline.length === 0) continue;
    ctx.save();
    ctx.strokeStyle = selection.has(v.id) ? layerColor("selection") : layerColor("LAYER_VIA_STITCHING");
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    // `DrawPolyline( outline )`: the chain as it is held (a closed one comes back to its start).
    ctx.lineWidth = Math.max(OUTLINE_WIDTH_NM, hairlineNm(view, 1));
    ctx.beginPath();
    v.outline.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
    ctx.closePath();
    ctx.stroke();
    ctx.lineWidth = hairlineNm(view, 1);
    ctx.beginPath();
    for (const [a, b] of stitchTicks(v.outline)) {
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
    }
    ctx.stroke();
    ctx.restore();
  }
}
