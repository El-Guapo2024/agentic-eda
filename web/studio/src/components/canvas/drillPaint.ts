// KiCad 10's drill charts and drill maps on the canvas (kicad-port/drillChart.ts): `PCB_PAINTER::drawChartSymbols` -- the shape mark of each row of a chart, in its symbol cell, in
// the layer's colour and the profile's mark width (the table itself is drawn as any table) -- and `draw( const PCB_DRILL_MAP* )` with `drawDrillMarks`: the board outline slid by
// the map's offset, and at each hole of the span it shows, slid by the same offset, the hole's mark (a curated shape, or its letter or size in the stroke font), the guide cross
// when the map has one, a slot's outline when it outlines slots; a selected map is boxed with a dashed line. KiCad draws the marks from the holes (`drawDrillSymbol`); here the map
// draws them, which is what KiCad does while a map is being dragged.
import type { BoardState } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { buildMarker, chartSymbols, curatedShape, DEFAULT_PROFILE, mapBounds, mapMarks, markText, outlineBox, outlineRings, type MarkerPart } from "../../kicad-port/drillChart";
import { layerIsVisible } from "../../kicad-port/layerPresets";
import { drawStrokeText } from "../text/strokeFont";
import { layerColor } from "./layers";
import { realLayerKey } from "./painter";
import { hairlineNm } from "./view";

function strokeParts(ctx: CanvasRenderingContext2D, cx: number, cy: number, parts: readonly MarkerPart[]): void {
  ctx.beginPath();
  for (const p of parts) {
    if (p.kind === "circle") {
      ctx.moveTo(cx + p.r, cy);
      ctx.arc(cx, cy, p.r, 0, Math.PI * 2);
    } else {
      p.points.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    }
  }
  ctx.stroke();
}

export function drawDrillItems(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, layerVisible: Readonly<Record<string, boolean>>, selection: ReadonlySet<string>): void {
  const dr = board.drawings;
  if (!dr) return;
  const profile = dr.drill_profile ?? DEFAULT_PROFILE;
  const width = Math.max(profile.width_nm, hairlineNm(view, 1));
  ctx.save();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  // The charts' shape marks.
  for (const chart of dr.drill_charts ?? []) {
    const table = (dr.tables ?? []).find((t) => t.id === chart.table_id);
    if (!table || !layerIsVisible(layerVisible, table.layer)) continue;
    ctx.strokeStyle = selection.has(table.id) ? layerColor("selection") : layerColor(realLayerKey(table.layer));
    ctx.lineWidth = width;
    for (const s of chartSymbols(chart, table, profile)) strokeParts(ctx, s.x, s.y, buildMarker(s.x, s.y, s.r, s.shape));
  }
  // The maps.
  const rings = outlineRings(board);
  for (const map of dr.drill_maps ?? []) {
    if (!layerIsVisible(layerVisible, map.layer)) continue;
    const color = selection.has(map.id) ? layerColor("selection") : layerColor(realLayerKey(map.layer));
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    // "The plotted width, not the sketch width, because the outline is artwork the map really does emit"
    ctx.lineWidth = Math.max(profile.width_nm, 1, hairlineNm(view, 1));
    ctx.beginPath();
    for (const ring of rings.filter((r) => r.length > 1)) {
      ring.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x + map.offset.x, y + map.offset.y) : ctx.lineTo(x + map.offset.x, y + map.offset.y)));
      ctx.closePath();
    }
    ctx.stroke();
    const r = Math.trunc(map.size_nm / 2);
    for (const m of mapMarks(map, dr.drill_marks ?? [])) {
      ctx.lineWidth = width;
      if (m.mode === "shape") strokeParts(ctx, m.x, m.y, buildMarker(m.x, m.y, r, curatedShape(m.shape_index)));
      else drawStrokeText(ctx, markText(m), m.x, m.y + map.size_nm / 2, { sizeNm: map.size_nm, thicknessNm: profile.width_nm, justify: "center", color });
      if (map.guide_cross) {
        // "Reach matches the plotter exactly rather than 2 * radius"
        const reach = map.size_nm;
        ctx.beginPath();
        ctx.moveTo(m.x - reach, m.y);
        ctx.lineTo(m.x + reach, m.y);
        ctx.moveTo(m.x, m.y - reach);
        ctx.lineTo(m.x, m.y + reach);
        ctx.stroke();
      }
      // "A slot's true extent is the cost driver, so it is outlined as well as marked": the oval the plotter builds, its long axis along the slot.
      if (map.outline_slots !== false && m.is_slot) {
        let [w, h] = m.size_nm;
        let angle = m.orientation_deg;
        if (w > h) {
          [w, h] = [h, w];
          angle += 90;
        }
        const half = Math.trunc((h - w) / 2);
        const rad = (-angle * Math.PI) / 180;
        const [s, c] = [Math.sin(rad), Math.cos(rad)];
        const rot = (x: number, y: number): [number, number] => [m.x + x * c - y * s, m.y + x * s + y * c];
        const [a, b] = [rot(0, half), rot(0, -half)];
        ctx.lineWidth = width;
        ctx.beginPath();
        const ang = Math.atan2(b[1] - a[1], b[0] - a[0]);
        ctx.arc(a[0], a[1], w / 2, ang + Math.PI / 2, ang + (3 * Math.PI) / 2);
        ctx.arc(b[0], b[1], w / 2, ang - Math.PI / 2, ang + Math.PI / 2);
        ctx.closePath();
        ctx.stroke();
      }
    }
    if (selection.has(map.id)) {
      const box = mapBounds(map, dr.drill_marks ?? [], profile, outlineBox(board));
      if (box) {
        ctx.save();
        ctx.lineWidth = hairlineNm(view, 1);
        ctx.setLineDash([hairlineNm(view, 6), hairlineNm(view, 4)]);
        ctx.strokeRect(box[0], box[1], box[2] - box[0], box[3] - box[1]);
        ctx.restore();
      }
    }
  }
  ctx.restore();
}
