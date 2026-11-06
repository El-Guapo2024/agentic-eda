// A footprint's graphics and texts on the canvas: `PCB_PAINTER::draw( PCB_SHAPE )` and `draw( PCB_TEXT )` for the items KiCad parents to a footprint. Each is drawn on its own
// layer, in that layer's colour, under the Appearance panel's rows -- the layer's switch and opacity, Footprints Front / Back, Footprint Text, References and Values
// (kicad-port/fpArt.ts) -- after the footprint's pads, so a pad's copper lies under the silkscreen that passes over it. They are in board space (the backend placed them), so the
// transform of a footprint in the hand (painter.ts `drawFootprint`) carries them with the rest.
//
// The shapes are drawn by the painter's own shape code (the board's graphics are the same `Shape`s, their dashes too). A hatched fill is drawn as its outline and
// the lines `EDA_SHAPE::UpdateHatching` makes of it, which the server works out from the board as it is (`hatch_lines`, UI-LEFT P15), as a board graphic's. A text is drawn as
// `FONT::Draw` lays it out (kicad-port/kicadText.ts through `fieldSpec`: the lines an interline apart, `GetInterline`), in the stroke font or its outline face; a knockout
// text is its box filled with the text cut out (UI-LEFT-2 PP1, CC1).

import type { Part } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { paintKicadText } from "../text/kicadTextPaint";
import { fieldSpec } from "../../kicad-port/textSpecs";
import { layerStateKey, isCopper } from "../../kicad-port/layerPresets";
import { keptReadableInFlippedView } from "../../kicad-port/flipView";
import { artLayerShown, artTextAsField, artTextShown, graphicsOf, layerRank, layersOf, type ArtShape } from "../../kicad-port/fpArt";
import { opacityOf } from "./appearancePaint";
import { layerColor } from "./layers";
import { drawHatchLines, drawShapeGeometry, realLayerKey, shapeDash, withLayer, type PaintOptions } from "./painter";
import { hairlineNm } from "./view";

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
    withLayer(ctx, opts, layerStateKey(layer), 1, () => {
      ctx.save();
      // A filled shape takes the Filled Shapes opacity, a graphic on copper the track opacity (`PCB_RENDER_SETTINGS::GetColor`), as the board's own shapes do.
      ctx.globalAlpha *= s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier" && s.filled ? opacityOf(opts.appearance, "shapes") : isCopper(layer) ? opacityOf(opts.appearance, "tracks") : 1;
      ctx.strokeStyle = selected ? layerColor("selection") : layerColor(realLayerKey(layer));
      ctx.fillStyle = ctx.strokeStyle;
      ctx.lineWidth = Math.max(s.stroke_width, hairlineNm(view, selected ? 2 : 1));
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      // A dashed graphic is cut as `STROKE_PARAMS::Stroke` cuts it (kicad-port/strokeDash.ts), piece by piece, with the board's dash ratios.
      drawShapeGeometry(ctx, s, !!opts.sketchGraphics, shapeDash(s, ctx.lineWidth, opts.dashRatios));
      // a hatched fill's lines (`PCB_PAINTER::draw( PCB_SHAPE )`: `GetHatchLines()`, at the shape's line width)
      drawHatchLines(ctx, s);
      ctx.restore();
    });
  }

  for (const t of [...texts].sort((a, b) => layerRank(a.layer) - layerRank(b.layer))) {
    if (!t.text || !artTextShown(opts.layerVisible, part, t)) continue;
    // the footprint selected, or the text on its own (UI-LEFT P15)
    const color = selected || opts.selection.has(t.id) ? layerColor("selection") : layerColor(realLayerKey(t.layer));
    withLayer(ctx, opts, layerStateKey(t.layer), 1, () => drawArtText(ctx, view, t, color, !!opts.sketchText, opts.viewFlipped));
  }
}

/** One text where `FONT::Draw` puts it (`fieldSpec`), the hit test's layout (kicad-port/textHit.ts). */
function drawArtText(ctx: CanvasRenderingContext2D, view: ViewTransform, t: NonNullable<Part["texts"]>[number], color: string, sketch: boolean, viewFlipped = false): void {
  const { spec, face } = fieldSpec(artTextAsField(t));
  paintKicadText(ctx, spec, face, {
    color,
    pen: sketch ? hairlineNm(view, 1) : undefined,
    minPen: hairlineNm(view, 0.5),
    knockout: t.knockout ? layerColor("background") : null,
    flippedView: keptReadableInFlippedView(viewFlipped, t.layer),
    record: { tag: "pcb", id: t.id },
  });
}
