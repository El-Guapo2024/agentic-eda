// Canvas2D drawing of the schematic's drawn graphics (`SchGraphic`): rectangles, circles, arcs, beziers,
// polygons, text boxes, rule areas and directive labels -- ported from `SCH_PAINTER::draw( SCH_SHAPE )`,
// `draw( SCH_TEXTBOX )` and `draw( SCH_DIRECTIVE_LABEL )` (eeschema/sch_painter.cpp at 8303b2ad) and the dash
// lengths of `RENDER_SETTINGS::GetDashLength/GetGapLength/GetDotLength`.
import type { SchColor, SchGraphic, SchLineStyle } from "../../api/schEditTypes";
import type { ViewTransform } from "../../state/store";
import { schImageUrl } from "../../kicad-port/refImage";
import { arcFromThreePoints, directiveShape, graphicBounds, pt, rectCorners, schImageBox } from "../../kicad-port/schItemGeom";
import { hatchLines } from "../../kicad-port/schHatch";
import { imageFor } from "../canvas/imageCache";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import { defaultTextBoxMargin, layoutTextBox } from "../../kicad-port/schTextBox";
import { layerColor } from "../canvas/layers";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";

/** `DEFAULT_LINE_WIDTH_MILS` (6 mil), um: the width of a stroke with none of its own. */
const DEFAULT_WIDTH_NM = 152_400;

export function rgba(c: SchColor): string {
  return `rgba(${c.r},${c.g},${c.b},${(c.a / 255).toFixed(3)})`;
}

/** The dash pattern for a line style at stroke width `w` (`correction` 1.0: dash 11w, gap 4w, dot 0.2w). */
export function dashPattern(style: SchLineStyle | undefined, w: number): number[] {
  const dash = 11 * w;
  const gap = 4 * w;
  const dot = 0.2 * w;
  switch (style) {
    case "dash":
      return [dash, gap];
    case "dot":
      return [dot, gap];
    case "dash_dot":
      return [dash, gap, dot, gap];
    case "dash_dot_dot":
      return [dash, gap, dot, gap, dot, gap];
    default:
      return [];
  }
}

function strokeColor(g: SchGraphic, selected: boolean): string {
  if (selected) return layerColor("LAYER_SELECTION_SHADOWS");
  if (g.color) return rgba(g.color);
  if (g.shape.type === "rule_area") return layerColor("LAYER_RULE_AREAS");
  if (g.shape.type === "directive") return layerColor("LAYER_NETCLASS_REFS");
  return layerColor("LAYER_NOTES");
}

function tracePath(ctx: CanvasRenderingContext2D, g: SchGraphic): void {
  const s = g.shape;
  ctx.beginPath();
  switch (s.type) {
    case "rectangle":
    case "text_box": {
      const c = rectCorners(pt(s.start), pt(s.end));
      c.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
      break;
    }
    case "circle":
      ctx.arc(s.center.x, s.center.y, s.radius_nm, 0, Math.PI * 2);
      break;
    case "arc": {
      const arc = arcFromThreePoints(pt(s.start), pt(s.mid), pt(s.end));
      if (!arc) {
        ctx.moveTo(s.start.x, s.start.y);
        ctx.lineTo(s.end.x, s.end.y);
      } else {
        ctx.arc(arc.center[0], arc.center[1], arc.radius, arc.startAngle, arc.startAngle + arc.sweep, arc.sweep < 0);
      }
      break;
    }
    case "bezier": {
      const pts = bezierPolyline(pt(s.start), pt(s.c1), pt(s.c2), pt(s.end));
      pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      break;
    }
    case "polygon":
    case "rule_area":
      s.pts.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
      ctx.closePath();
      break;
    case "directive":
      break;
  }
}

function drawDirective(ctx: CanvasRenderingContext2D, view: ViewTransform, g: SchGraphic, color: string): void {
  const s = g.shape;
  if (s.type !== "directive") return;
  const d = directiveShape(pt(s.at), (s.orientation ?? 0) / 1000, s.shape ?? "round", s.pin_length_nm);
  const hair = 1 / view.scale;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = Math.max(DEFAULT_WIDTH_NM, hair);
  const shape = s.shape ?? "round";
  ctx.beginPath();
  if (shape === "dot" || shape === "round") {
    ctx.moveTo(d.outline[0]![0], d.outline[0]![1]);
    ctx.lineTo(d.outline[1]![0], d.outline[1]![1]);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(d.flagCenter[0], d.flagCenter[1], d.flagRadius, 0, Math.PI * 2);
    if (shape === "dot") ctx.fill();
    ctx.stroke();
  } else {
    d.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
  }
  // The fields (`Netclass`, `Component Class`) stack along the pole, drawn small beside the flag.
  const lines = [s.netclass, s.component_class].filter((t): t is string => Boolean(t));
  const size = 1_270_000;
  const spin = (((Math.round((s.orientation ?? 0) / 1000) % 360) + 360) % 360) as number;
  lines.forEach((text, i) => {
    const off = 1_000_000 + i * size * 1.4;
    const [x, y] = d.flagCenter;
    if (spin === 0 || spin === 180) drawStrokeText(ctx, text, x + off, y + size * 0.35, { sizeNm: size, justify: "left", color });
    else drawStrokeText(ctx, text, x + size * 0.35, y - off, { sizeNm: size, angleRad: -Math.PI / 2, justify: "left", color });
  });
}

function drawTextBoxText(ctx: CanvasRenderingContext2D, g: SchGraphic, color: string): void {
  const s = g.shape;
  if (s.type !== "text_box") return;
  const widthNm = g.width_nm && g.width_nm > 0 ? g.width_nm : DEFAULT_WIDTH_NM;
  const layout = layoutTextBox(
    {
      start: [s.start.x, s.start.y],
      end: [s.end.x, s.end.y],
      text: s.text,
      angle: s.angle ?? 0,
      sizeNm: s.size_nm,
      hAlign: s.h_align ?? "left",
      vAlign: s.v_align ?? "top",
      marginNm: s.margin_nm && s.margin_nm > 0 ? s.margin_nm : defaultTextBoxMargin(s.size_nm, widthNm),
    },
    measureStrokeText
  );
  ctx.save();
  ctx.translate(layout.origin[0], layout.origin[1]);
  if (layout.vertical) ctx.rotate(-Math.PI / 2);
  for (const l of layout.lines) drawStrokeText(ctx, l.text, l.u, l.v, { sizeNm: s.size_nm, justify: l.justify, italic: s.italic ?? false, thicknessNm: s.bold ? s.size_nm / 5 : undefined, color });
  ctx.restore();
}

/**
 * Paint every graphic, in drawing order. `selection`: ids drawn in the selection colour. Fills go first (a filled
 * shape's body, then its outline), the way `SCH_PAINTER::draw( SCH_SHAPE )` layers the background and the stroke.
 */
export function paintGraphics(ctx: CanvasRenderingContext2D, view: ViewTransform, graphics: readonly SchGraphic[], selection: ReadonlySet<string>): void {
  const hair = 1 / view.scale;
  for (const g of graphics) {
    // An image is painted under everything else (`paintImages`).
    if (g.shape.type === "image") continue;
    const selected = selection.has(g.id);
    const color = strokeColor(g, selected);
    ctx.save();
    if (g.shape.type === "directive") {
      drawDirective(ctx, view, g, color);
      ctx.restore();
      continue;
    }
    const widthNm = g.width_nm && g.width_nm > 0 ? g.width_nm : DEFAULT_WIDTH_NM;
    const fill = g.fill ?? "none";
    if ((fill === "hatch" || fill === "reverse_hatch" || fill === "cross_hatch") && g.shape.type !== "arc" && g.shape.type !== "bezier") {
      // `SCH_PAINTER::draw( SCH_SHAPE )`: the hatch lines in the background layer's colour (the fill colour, else the stroke's), clipped to the shape (r7 M5)
      tracePath(ctx, g);
      ctx.save();
      ctx.clip();
      const b = graphicBounds(g);
      const h = hatchLines(fill, [b.minX, b.minY, b.maxX, b.maxY], widthNm);
      ctx.strokeStyle = g.fill_color ? rgba(g.fill_color) : strokeColor(g, false);
      ctx.lineWidth = Math.max(h.width, hair);
      ctx.beginPath();
      for (const [ax, ay, bx, by] of h.segments) {
        ctx.moveTo(ax, ay);
        ctx.lineTo(bx, by);
      }
      ctx.stroke();
      ctx.restore();
    } else if (fill !== "none" && g.shape.type !== "arc" && g.shape.type !== "bezier") {
      tracePath(ctx, g);
      ctx.fillStyle = fill === "color" && g.fill_color ? rgba(g.fill_color) : fill === "background" ? layerColor("LAYER_NOTES_BACKGROUND") : strokeColor(g, false);
      ctx.fill();
    }
    // A rule area is always shown with a faint body so it reads as an area (the creation preview's own 0.2 alpha).
    if (g.shape.type === "rule_area" && fill === "none") {
      tracePath(ctx, g);
      ctx.globalAlpha = 0.08;
      ctx.fillStyle = color;
      ctx.fill();
      ctx.globalAlpha = 1;
    }
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(selected ? widthNm * 1.6 : widthNm, hair * (selected ? 2.5 : 1));
    const dash = g.shape.type === "rule_area" && !g.line_style ? dashPattern("dash", widthNm) : dashPattern(g.line_style, widthNm);
    ctx.setLineDash(dash);
    tracePath(ctx, g);
    ctx.stroke();
    ctx.setLineDash([]);
    if (g.shape.type === "text_box") drawTextBoxText(ctx, g, selected ? color : layerColor("LAYER_NOTES"));
    ctx.restore();
  }
}

/**
 * `SCH_PAINTER::draw( const SCH_BITMAP* )` on `LAYER_DRAW_BITMAPS`, the lowest layer of the sheet: each image at its place and size, opaque, and while it is selected a rectangle round it in the
 * selection colour. `sheet` is the `/`-joined path of the sheet in view, which `GET /api/sch_image` needs to find the file. Until the bitmap has arrived its box is drawn dashed so it can be picked.
 */
export function paintImages(ctx: CanvasRenderingContext2D, view: ViewTransform, graphics: readonly SchGraphic[], selection: ReadonlySet<string>, sheet: string): void {
  const hair = 1 / view.scale;
  for (const g of graphics) {
    const s = g.shape;
    if (s.type !== "image") continue;
    const [x0, y0, x1, y1] = schImageBox(s);
    const el = imageFor(schImageUrl(g.id, s.hash ?? "", sheet));
    ctx.save();
    if (el) {
      ctx.drawImage(el, x0, y0, x1 - x0, y1 - y0);
    } else {
      ctx.strokeStyle = layerColor("LAYER_NOTES");
      ctx.lineWidth = Math.max(hair, 100_000);
      ctx.setLineDash([4 / view.scale, 3 / view.scale]);
      ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
    }
    if (selection.has(g.id)) {
      ctx.setLineDash([]);
      ctx.strokeStyle = layerColor("LAYER_SELECTION_SHADOWS");
      ctx.lineWidth = Math.max(hair * 2.5, 300_000);
      ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
    }
    ctx.restore();
  }
}

/** A dashed rectangle around a selected item whose own colour change is too faint to read (text, labels, sheets...). */
export function drawSelectionBox(ctx: CanvasRenderingContext2D, view: ViewTransform, box: { minX: number; minY: number; maxX: number; maxY: number }): void {
  const hair = 1 / view.scale;
  ctx.save();
  ctx.strokeStyle = layerColor("LAYER_SELECTION_SHADOWS");
  ctx.lineWidth = Math.max(hair * 1.5, 100_000);
  ctx.setLineDash([6 / view.scale, 4 / view.scale]);
  ctx.strokeRect(box.minX - 200_000, box.minY - 200_000, box.maxX - box.minX + 400_000, box.maxY - box.minY + 400_000);
  ctx.restore();
}

