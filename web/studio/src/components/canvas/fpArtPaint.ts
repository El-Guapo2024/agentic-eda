// A footprint's graphics and texts on the canvas: `PCB_PAINTER::draw( PCB_SHAPE )` and `draw( PCB_TEXT )` for the items KiCad parents to a footprint. Each is drawn on its own
// layer, in that layer's colour, under the Appearance panel's rows -- the layer's switch and opacity, Footprints Front / Back, Footprint Text, References and Values
// (kicad-port/fpArt.ts) -- after the footprint's pads, so a pad's copper lies under the silkscreen that passes over it. They are in board space (the backend placed them), so the
// transform of a footprint in the hand (painter.ts `drawFootprint`) carries them with the rest.
//
// The shapes are drawn by the painter's own shape code (the board's graphics are the same `Shape`s); this adds the stroke's dashes. A hatched fill is drawn as its outline
// (the hatch lines of `EDA_SHAPE::UpdateHatching` are not ported). A text is drawn in the stroke font a line at a time (`FONT::Draw` steps a multi-line text down by the interline,
// `STROKE_FONT::GetInterline`); a knockout text is drawn plain.

import type { Part } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { drawStrokeText } from "../text/strokeFont";
import { layerStateKey, isCopper } from "../../kicad-port/layerPresets";
import { artLayerShown, artTextAsField, artTextShown, dashPattern, graphicsOf, layerRank, layersOf, type ArtShape } from "../../kicad-port/fpArt";
import { fieldAsText } from "../../kicad-port/fpFields";
import { opacityOf } from "./appearancePaint";
import { layerColor } from "./layers";
import { drawShapeGeometry, layerAlpha, realLayerKey, withAlpha, type PaintOptions } from "./painter";
import { hairlineUm } from "./view";

/** `STROKE_FONT::GetInterline`: `FONT_METRICS`' 1.62 glyph heights, times the legacy factor 0.9583 that keeps the old spacing. */
const INTERLINE = 1.62 * 0.9583;

/** Draw the graphics and texts of `part`; nothing when it has none, or its side is not shown. */
export function drawFootprintArt(ctx: CanvasRenderingContext2D, view: ViewTransform, part: Part, opts: PaintOptions): void {
  const shapes = graphicsOf(part);
  const texts = part.texts ?? [];
  if (!shapes.length && !texts.length) return;
  const selected = opts.selection.has(part.ref);

  // Back to front, layer by layer: the fab drawings under the silkscreen, a user layer under both.
  const items: { rank: number; layer: string; shape: ArtShape }[] = [];
  for (const shape of shapes) {
    for (const layer of layersOf(shape)) {
      if (artLayerShown(opts.layerVisible, part, layer)) items.push({ rank: layerRank(layer), layer, shape });
    }
  }
  items.sort((a, b) => a.rank - b.rank);
  for (const { layer, shape: s } of items) {
    withAlpha(ctx, layerAlpha(opts, layerStateKey(layer)), () => {
      ctx.save();
      // A filled shape takes the Filled Shapes opacity, a graphic on copper the track opacity (`PCB_RENDER_SETTINGS::GetColor`), as the board's own shapes do.
      ctx.globalAlpha *= s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier" && s.filled ? opacityOf(opts.appearance, "shapes") : isCopper(layer) ? opacityOf(opts.appearance, "tracks") : 1;
      ctx.strokeStyle = selected ? layerColor("selection") : layerColor(realLayerKey(layer));
      ctx.fillStyle = ctx.strokeStyle;
      ctx.lineWidth = Math.max(s.stroke_width, hairlineUm(view, selected ? 2 : 1));
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      if (s.dash) ctx.setLineDash(dashPattern(s.dash, s.stroke_width));
      drawShapeGeometry(ctx, s, !!opts.sketchGraphics);
      ctx.restore();
    });
  }

  for (const t of [...texts].sort((a, b) => layerRank(a.layer) - layerRank(b.layer))) {
    if (!t.text || !artTextShown(opts.layerVisible, part, t)) continue;
    const color = selected ? layerColor("selection") : layerColor(realLayerKey(t.layer));
    withAlpha(ctx, layerAlpha(opts, layerStateKey(t.layer)), () => drawArtText(ctx, view, t, color, !!opts.sketchText));
  }
}

/** One text, a line at a time: the lines step down the text's own axis by the interline, the block placed on the anchor by the vertical justification. */
function drawArtText(ctx: CanvasRenderingContext2D, view: ViewTransform, t: NonNullable<Part["texts"]>[number], color: string, sketch: boolean): void {
  const text = fieldAsText(artTextAsField(t));
  const lines = t.text.split("\n");
  const sizeUm = Math.max(t.h, hairlineUm(view, 8));
  // KiCad's counter-clockwise millidegrees; the canvas turns clockwise.
  const angleRad = (-text.angle / 1000) * (Math.PI / 180);
  const interline = INTERLINE * sizeUm;
  // The first baseline below the anchor: the glyphs' bottom is on it, 0.72 of the size below the top and 0.36 below the middle of a line; a block of several lines
  // hangs from the top, is centred on the anchor or ends on it.
  const block = (lines.length - 1) * interline;
  const first = t.valign > 0 ? -block : t.valign < 0 ? 0.72 * sizeUm : 0.36 * sizeUm - block / 2;
  const pen = t.thickness > 0 ? t.thickness : t.bold ? sizeUm / 5 : sizeUm / 8;
  lines.forEach((line, i) => {
    if (!line) return;
    const down = first + i * interline;
    drawStrokeText(ctx, line, t.x - down * Math.sin(angleRad), t.y + down * Math.cos(angleRad), {
      sizeUm,
      thicknessUm: sketch ? hairlineUm(view, 1) : Math.min(Math.max(pen, hairlineUm(view, 0.5)), sizeUm / 4),
      justify: text.justify,
      angleRad,
      mirror: t.mirror,
      italic: t.italic,
      color,
    });
  });
}
